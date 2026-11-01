# -*- coding: utf-8 -*-
"""
사용자 지정 폴더 말뭉치 자동 감지 및 맞춤형 DB 구축 엔진 (corpus_builder.py)
- 지원 유형:
  1) 세종 3중 연동 (BTAA + BSAA + BGAA)
  2) 학습자 연동 (txt + xml) 및 단독 학습자 XML
  3) 옛한글 사전/자석류 (<c><g><m><s> 또는 훈몽자회 형식)
  4) 옛한글 일반 언해문/대역류 (<sent type lang page n>)
  5) 모두의 말뭉치 JSON (형태 분석 / 원시 문단)
  6) 세종 TEI 텍스트 (문어 2BA*, 구어 4CM*, 구어형태 5CT*)
  7) 대본/희곡 및 일반 평문 텍스트
- 모듈형 .corpdb 생성 및 기존 concordance.corpdb 증분 병합/새성 모두 지원
"""
import os
import sys
import re
import json
import sqlite3
import unicodedata
import xml.etree.ElementTree as ET
from datetime import datetime
from pathlib import Path

# 기본 작업 경로 (실행 위치 무관 동적 감지)
WORKSPACE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_DB_PATH = os.path.join(WORKSPACE, "db", "concordance.corpdb")
PUA_TABLE_PATH = os.path.join(WORKSPACE, "assets", "data", "pua_table.json")
if not os.path.exists(PUA_TABLE_PATH):
    PUA_TABLE_PATH = os.path.join(
        WORKSPACE, "HNI_MMK_Kug_1.2", "Old_Hangul", "program", "app", "lib", "pua_table.json"
    )

PUA_TO_NFD = {}
if os.path.exists(PUA_TABLE_PATH):
    try:
        with open(PUA_TABLE_PATH, "r", encoding="utf-8") as f:
            for hex_code, nfd_str in json.load(f).items():
                PUA_TO_NFD[chr(int(hex_code, 16))] = nfd_str
    except Exception as e:
        print(f"[Warning] Failed to load pua_table: {e}", file=sys.stderr)

COMPAT_TO_NFD = {
    'ㄱ': '\u1100', 'ㄲ': '\u1101', 'ㄴ': '\u1102', 'ㄷ': '\u1103', 'ㄸ': '\u1104',
    'ㄹ': '\u1105', 'ㅁ': '\u1106', 'ㅂ': '\u1107', 'ㅃ': '\u1108', 'ㅅ': '\u1109',
    'ㅆ': '\u110a', 'ㅇ': '\u110b', 'ㅈ': '\u110c', 'ㅉ': '\u110d', 'ㅊ': '\u110e',
    'ㅋ': '\u110f', 'ㅌ': '\u1110', 'ㅍ': '\u1111', 'ㅎ': '\u1112',
    'ㅥ': '\u1114', 'ㅦ': '\u1115', 'ㅧ': '\u1117', 'ㅨ': '\u1118', 'ㅩ': '\u111a',
    'ㅪ': '\u111b', 'ㅫ': '\u111c', 'ㅬ': '\u111d', 'ㅭ': '\u111c', 'ㅮ': '\u111e',
    'ㅯ': '\u111f', 'ㅰ': '\u1120', 'ㅱ': '\u111d', 'ㅲ': '\u1120', 'ㅳ': '\u1121',
    'ㅴ': '\u1122', 'ㅵ': '\u1123', 'ㅶ': '\u1127', 'ㅷ': '\u1129', 'ㅸ': '\u112b',
    'ㅹ': '\u112c', 'ㅺ': '\u112d', 'ㅼ': '\u112e', 'ㅽ': '\u112f', 'ㅾ': '\u1132',
    'ㅿ': '\u1140', 'ㆀ': '\u1147', 'ㆁ': '\u114c', 'ㆂ': '\u1152', 'ㆃ': '\u1153',
    'ㆄ': '\u1157', 'ㆅ': '\u1158', 'ㆆ': '\u1159',
    'ㅏ': '\u1161', 'ㅐ': '\u1162', 'ㅑ': '\u1163', 'ㅒ': '\u1164', 'ㅓ': '\u1165',
    'ㅔ': '\u1166', 'ㅕ': '\u1167', 'ㅖ': '\u1168', 'ㅗ': '\u1169', 'ㅘ': '\u116a',
    'ㅙ': '\u116b', 'ㅚ': '\u116c', 'ㅛ': '\u116d', 'ㅜ': '\u116e', 'ㅝ': '\u116f',
    'ㅞ': '\u1170', 'ㅟ': '\u1171', 'ㅠ': '\u1172', 'ㅡ': '\u1173', 'ㅢ': '\u1174',
    'ㅣ': '\u1175', 'ㆍ': '\u119e', 'ᆢ': '\u11a2', 'ㆎ': '\u11a1', 'ㆇ': '\u1184',
    'ㆈ': '\u1185', 'ㆉ': '\u1188', 'ㆊ': '\u1191', 'ㆋ': '\u1192', 'ㆌ': '\u1194',
}

def pua_to_unicode(text: str) -> str:
    if not text:
        return ""
    return "".join(PUA_TO_NFD.get(ch, ch) for ch in text)

def to_nfd_norm(text: str) -> str:
    if not text:
        return ""
    u_text = pua_to_unicode(text)
    nfd = unicodedata.normalize("NFD", u_text)
    out = []
    for ch in nfd:
        cp = ord(ch)
        if cp in (0x115F, 0x1160):
            continue
        out.append(COMPAT_TO_NFD.get(ch, ch))
    return "".join(out)

def detect_file_encoding(file_path: str):
    with open(file_path, "rb") as f:
        head = f.read(4096)
    if head.startswith(b"\xff\xfe"):
        return "utf-16-le", "UTF-16LE (with BOM)"
    if head.startswith(b"\xfe\xff"):
        return "utf-16-be", "UTF-16BE (with BOM)"
    if head.startswith(b"\xef\xbb\xbf"):
        return "utf-8-sig", "UTF-8-SIG (with BOM)"
    # Try utf-8
    try:
        head.decode("utf-8")
        return "utf-8", "UTF-8"
    except UnicodeDecodeError:
        pass
    # Try cp949
    try:
        head.decode("cp949")
        return "cp949", "CP949 / EUC-KR"
    except UnicodeDecodeError:
        return "utf-8-sig", "UTF-8"

def read_sample_text(file_path: str, max_bytes=8192):
    enc, label = detect_file_encoding(file_path)
    try:
        with open(file_path, "r", encoding=enc, errors="ignore") as f:
            return f.read(max_bytes), enc, label
    except Exception:
        return "", enc, label

def init_db(conn: sqlite3.Connection):
    cur = conn.cursor()
    cur.executescript("""
    CREATE TABLE IF NOT EXISTS documents (
        doc_id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        author TEXT,
        pub_year TEXT,
        corpus_category TEXT NOT NULL,
        annotation_layers TEXT NOT NULL,
        source_files_list TEXT NOT NULL,
        storage_mode TEXT NOT NULL DEFAULT 'ARCHIVED',
        sentence_count INTEGER NOT NULL DEFAULT 0,
        eojeol_count INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS source_files (
        file_id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_name TEXT NOT NULL,
        rel_path TEXT NOT NULL,
        file_size_bytes INTEGER NOT NULL,
        original_encoding TEXT NOT NULL,
        text_format TEXT NOT NULL,
        corpus_type TEXT NOT NULL,
        raw_notation_desc TEXT NOT NULL,
        analysis_notation_desc TEXT NOT NULL,
        linked_doc_id TEXT NOT NULL,
        db_file_name TEXT NOT NULL,
        imported_sentences INTEGER NOT NULL,
        imported_eojeols INTEGER NOT NULL,
        note TEXT,
        FOREIGN KEY (linked_doc_id) REFERENCES documents(doc_id)
    );

    CREATE TABLE IF NOT EXISTS segments (
        segment_id INTEGER PRIMARY KEY AUTOINCREMENT,
        doc_id TEXT NOT NULL,
        seq_no INTEGER NOT NULL,
        sent_id_str TEXT NOT NULL,
        part_type TEXT,
        lang TEXT DEFAULT 'kor',
        page_info TEXT,
        text_raw TEXT NOT NULL,
        text_unicode TEXT NOT NULL,
        text_nfd_norm TEXT NOT NULL,
        morph_flat TEXT,
        sense_flat TEXT,
        syntax_tree TEXT,
        eojeols_json TEXT,
        is_modified INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY (doc_id) REFERENCES documents(doc_id)
    );

    CREATE TABLE IF NOT EXISTS segment_edits (
        segment_id INTEGER PRIMARY KEY,
        original_text_raw TEXT NOT NULL,
        original_morph_flat TEXT,
        edited_text_raw TEXT NOT NULL,
        edited_morph_flat TEXT,
        updated_at TEXT NOT NULL,
        synced_to_source INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY (segment_id) REFERENCES segments(segment_id)
    );

    CREATE INDEX IF NOT EXISTS idx_segments_doc ON segments(doc_id, seq_no);
    CREATE INDEX IF NOT EXISTS idx_segments_lang ON segments(lang);
    """)
    conn.commit()

def save_document_and_segments(conn, doc_info, file_records, segments_list, target_db_name="db/concordance.corpdb"):
    cur = conn.cursor()
    now_str = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    total_sents = len(segments_list)
    total_eojeols = sum(
        s["eojeol_cnt"] if "eojeol_cnt" in s else len((s.get("text_unicode") or s["text_raw"]).split())
        for s in segments_list
    )

    # 기존 doc_id가 있으면 삭제 후 갱신
    cur.execute("DELETE FROM segments WHERE doc_id = ?", (doc_info["doc_id"],))
    cur.execute("DELETE FROM source_files WHERE linked_doc_id = ?", (doc_info["doc_id"],))
    cur.execute("DELETE FROM documents WHERE doc_id = ?", (doc_info["doc_id"],))

    cur.execute("""
        INSERT INTO documents (
            doc_id, title, author, pub_year, corpus_category, annotation_layers,
            source_files_list, storage_mode, sentence_count, eojeol_count, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 'ARCHIVED', ?, ?, ?)
    """, (
        doc_info["doc_id"],
        doc_info["title"],
        doc_info.get("author", ""),
        doc_info.get("pub_year", ""),
        doc_info["corpus_category"],
        doc_info["annotation_layers"],
        ", ".join(r["file_name"] for r in file_records),
        total_sents,
        total_eojeols,
        now_str
    ))

    for r in file_records:
        cur.execute("""
            INSERT INTO source_files (
                file_name, rel_path, file_size_bytes, original_encoding, text_format,
                corpus_type, raw_notation_desc, analysis_notation_desc, linked_doc_id,
                db_file_name, imported_sentences, imported_eojeols, note
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """, (
            r["file_name"],
            r.get("rel_path", r["file_name"]),
            r["file_size_bytes"],
            r["original_encoding"],
            r["text_format"],
            r["corpus_type"],
            r["raw_notation_desc"],
            r["analysis_notation_desc"],
            doc_info["doc_id"],
            target_db_name,
            r.get("imported_sentences", total_sents),
            r.get("imported_eojeols", total_eojeols),
            r.get("note", "")
        ))

    rows = []
    for idx, s in enumerate(segments_list, 1):
        t_raw = s["text_raw"]
        t_uni = s.get("text_unicode") or pua_to_unicode(t_raw)
        t_nfd = to_nfd_norm(t_uni)
        rows.append((
            doc_info["doc_id"],
            idx,
            s.get("sent_id_str", f"{doc_info['doc_id']}-{idx:05d}"),
            s.get("part_type", "main"),
            s.get("lang", "kor"),
            s.get("page_info", ""),
            t_raw,
            t_uni,
            t_nfd,
            s.get("morph_flat", ""),
            s.get("sense_flat", ""),
            s.get("syntax_tree", ""),
            s.get("eojeols_json", ""),
        ))

    cur.executemany("""
        INSERT INTO segments (
            doc_id, seq_no, sent_id_str, part_type, lang, page_info,
            text_raw, text_unicode, text_nfd_norm,
            morph_flat, sense_flat, syntax_tree, eojeols_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """, rows)
    conn.commit()
    return total_sents, total_eojeols

# ==============================================================================
# 각 유형별 파서 구현
# ==============================================================================

# 1. 옛한글 사전/자석류 (<c><g><m><s> 또는 훈몽자회 형태)
def parse_dictionary_xml(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        lines = [l.rstrip("\r\n") for l in f]
    cur_vol = "권차"
    cur_page = "1a"
    segs = []
    c_idx = 0
    for l in lines:
        hm = re.search(r'<head\s+n="([^"]+)"\s*/>', l)
        if hm:
            cur_vol = hm.group(1)
            continue
        pm = re.search(r'<page\s+n="([^"]+)"\s*/>', l)
        if pm:
            cur_page = pm.group(1)
            continue
        cm = re.search(r'<c><g>(.*?)</g><m>(.*?)</m><s>(.*?)</s></c>', l)
        if cm:
            c_idx += 1
            hanja, hun_raw, eum_raw = cm.group(1), cm.group(2), cm.group(3)
            hun_uni = pua_to_unicode(hun_raw)
            eum_uni = pua_to_unicode(eum_raw)
            raw_line = f"{hanja} : [훈] {hun_raw} / [음] {eum_raw}"
            uni_line = f"{hanja} : [훈] {hun_uni} / [음] {eum_uni}"
            segs.append({
                "sent_id_str": f"{Path(file_path).stem}-{cur_vol}:{cur_page}-{c_idx:04d}",
                "part_type": "gloss",
                "lang": "kor+chi",
                "page_info": f"{cur_vol} {cur_page}",
                "text_raw": raw_line,
                "text_unicode": uni_line,
                "morph_flat": f"한자(g): {hanja} | 훈/새김(m): {hun_uni} | 동국정운/한자음(s): {eum_uni}",
                "syntax_tree": l.strip(),
                "eojeol_cnt": 3
            })
    return segs

# 2. 옛한글 일반 문헌/언해/학습서 (<sent ...> ... </sent>)
def parse_sent_xml(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        content = f.read()
    segs = []
    for idx, m in enumerate(re.finditer(r'<sent\s+([^>]+)>(.*?)</sent>', content, re.DOTALL), 1):
        attrs_str, body = m.group(1), m.group(2).strip()
        stype = re.search(r'type="([^"]+)"', attrs_str)
        slang = re.search(r'lang="([^"]+)"', attrs_str)
        spage = re.search(r'page="([^"]+)"', attrs_str)
        sn = re.search(r'n="([^"]+)"', attrs_str)
        stype_v = stype.group(1) if stype else "main"
        slang_v = slang.group(1) if slang else "kor"
        spage_v = spage.group(1) if spage else ""
        sn_v = sn.group(1) if sn else str(idx)
        segs.append({
            "sent_id_str": f"{Path(file_path).stem}-{spage_v}-{sn_v}",
            "part_type": stype_v,
            "lang": slang_v,
            "page_info": f"{spage_v} #{sn_v}".strip(),
            "text_raw": body,
            "text_unicode": pua_to_unicode(body),
            "syntax_tree": f"[문헌 메타] 유형: {stype_v} | 언어: {slang_v} | 장차: {spage_v} | 문장번호: {sn_v}"
        })
    return segs

# 3. 모두의 말뭉치 JSON (형태분석 또는 원시)
def parse_nikl_json(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        data = json.load(f)
    segs = []
    has_morph = False
    for doc in data.get("document", []):
        doc_id = doc.get("id", "")
        # 형태분석 문장 있는 경우
        if "sentence" in doc and doc["sentence"]:
            has_morph = True
            for sent in doc.get("sentence", []):
                form = sent.get("form", "").strip()
                if not form:
                    continue
                words = {w["id"]: {"id": str(w["id"]), "word": w["form"], "morphs": []} for w in sent.get("word", [])}
                for m in sent.get("morpheme", []):
                    wid = m.get("word_id")
                    if wid in words:
                        words[wid]["morphs"].append(f"{m['form']}/{m['label']}")
                eojeol_list = []
                morph_flat_parts = []
                for wid in sorted(words.keys()):
                    w_obj = words[wid]
                    m_str = " + ".join(w_obj["morphs"])
                    eojeol_list.append({"id": w_obj["id"], "word": w_obj["word"], "morph": m_str, "sense": ""})
                    morph_flat_parts.append(f"{w_obj['word']}[{m_str}]")
                segs.append({
                    "sent_id_str": sent.get("id", f"{doc_id}-{len(segs)+1}"),
                    "part_type": "sentence",
                    "lang": "kor",
                    "page_info": doc_id,
                    "text_raw": form,
                    "morph_flat": " | ".join(morph_flat_parts),
                    "eojeols_json": json.dumps(eojeol_list, ensure_ascii=False),
                    "eojeol_cnt": len(eojeol_list)
                })
        # 원시 문단 있는 경우
        elif "paragraph" in doc and doc["paragraph"]:
            d_meta = doc.get("metadata", {})
            d_date = d_meta.get("date", "")
            d_topic = d_meta.get("topic", "")
            for para in doc.get("paragraph", []):
                form = para.get("form", "").strip()
                if not form:
                    continue
                segs.append({
                    "sent_id_str": para.get("id", f"{doc_id}-P{len(segs)+1}"),
                    "part_type": d_topic or "paragraph",
                    "lang": "kor",
                    "page_info": d_date or doc_id,
                    "text_raw": form
                })
    return segs, has_morph

# 4. 세종 TEI 문어 원시 (2BA* 형태: <head>, <p>)
def parse_sejong_tei_written(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        lines = [l.rstrip("\r\n") for l in f]
    segs = []
    cur_date = ""
    cur_page = ""
    in_body = False
    idx = 0
    for l in lines:
        s = l.strip()
        if "<body>" in s:
            in_body = True
            continue
        if not in_body:
            continue
        dm = re.search(r"<date>(.*?)</date>", s)
        if dm:
            cur_date = dm.group(1).strip()
            continue
        pm = re.search(r"<page>(.*?)</page>", s)
        if pm:
            cur_page = pm.group(1).strip()
            continue
        hm = re.search(r"<head>(.*?)</head>", s)
        if hm:
            idx += 1
            segs.append({
                "sent_id_str": f"{Path(file_path).stem}-{idx:05d}",
                "part_type": "head",
                "lang": "kor",
                "page_info": f"{cur_date} p.{cur_page}".strip(),
                "text_raw": hm.group(1).strip()
            })
            continue
        pgm = re.search(r"<p>(.*?)</p>", s)
        if pgm:
            idx += 1
            segs.append({
                "sent_id_str": f"{Path(file_path).stem}-{idx:05d}",
                "part_type": "p",
                "lang": "kor",
                "page_info": f"{cur_date} p.{cur_page}".strip(),
                "text_raw": pgm.group(1).strip()
            })
    return segs

# 5. 세종 TEI 구어 전사 원시 (4CM* 형태: <u who> <s n>)
def parse_sejong_tei_spoken_raw(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        lines = [l.rstrip("\r\n") for l in f]
    segs = []
    cur_who = ""
    in_text = False
    for l in lines:
        if "<text>" in l:
            in_text = True
            continue
        if not in_text:
            continue
        um = re.search(r'<u\s+who="([^"]+)">', l)
        if um:
            cur_who = um.group(1)
        for sm in re.finditer(r'<s\s+n="([^"]+)">(.*?)</s>', l):
            s_num, raw_inner = sm.group(1), sm.group(2)
            clean_text = re.sub(r"<phon>(.*?)</phon>", r"(\1)", raw_inner)
            clean_text = re.sub(r"<[^>]+>", "", clean_text).strip()
            if clean_text:
                segs.append({
                    "sent_id_str": f"{Path(file_path).stem}-S{s_num}",
                    "part_type": f"발화({cur_who})",
                    "lang": "kor",
                    "page_info": f"화자 {cur_who}",
                    "text_raw": clean_text,
                    "syntax_tree": f"<u who=\"{cur_who}\"><s n=\"{s_num}\">{raw_inner}</s></u>"
                })
    return segs

# 6. 세종 구어 형태 분석 (5CT* 형태: 행ID \t 어절 \t 분석)
def parse_sejong_spoken_morph(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        lines = [l.rstrip("\r\n") for l in f]
    segs = []
    cur_who = ""
    cur_sn = ""
    cur_eojeols = []
    in_text = False
    for l in lines:
        if "<text>" in l:
            in_text = True
            continue
        if not in_text:
            continue
        if "\t" in l:
            parts = l.split("\t")
            line_id = parts[0]
            col1 = parts[1].strip()
            if col1.startswith("<u "):
                um = re.search(r'who="([^"]+)"', col1)
                if um: cur_who = um.group(1)
            elif col1.startswith("<s "):
                sm = re.search(r'n="([^"]+)"', col1)
                if sm: cur_sn = sm.group(1)
                cur_eojeols = []
            elif col1 == "</s>":
                if cur_eojeols:
                    raw_s = " ".join(x["word"] for x in cur_eojeols)
                    m_flat = " | ".join(f"{x['word']}[{x['morph']}]" for x in cur_eojeols)
                    segs.append({
                        "sent_id_str": f"{Path(file_path).stem}-S{cur_sn}",
                        "part_type": f"발화({cur_who})",
                        "lang": "kor",
                        "page_info": f"화자 {cur_who}",
                        "text_raw": raw_s,
                        "morph_flat": m_flat,
                        "eojeols_json": json.dumps(cur_eojeols, ensure_ascii=False),
                        "eojeol_cnt": len(cur_eojeols)
                    })
                    cur_eojeols = []
            elif len(parts) >= 3:
                cur_eojeols.append({
                    "id": line_id,
                    "word": col1,
                    "morph": parts[2].strip(),
                    "sense": ""
                })
    return segs

# 7. 일반 평문 및 대본
def parse_plain_or_script(file_path: str, enc: str):
    with open(file_path, "r", encoding=enc, errors="ignore") as f:
        lines = [l.rstrip("\r\n") for l in f]
    segs = []
    cur_scene = "본문"
    idx = 0
    for l in lines:
        s = l.strip()
        if not s:
            continue
        if s.startswith("#"):
            cur_scene = s[:30]
            continue
        idx += 1
        segs.append({
            "sent_id_str": f"{Path(file_path).stem}-{idx:05d}",
            "part_type": "text",
            "lang": "kor",
            "page_info": cur_scene,
            "text_raw": s
        })
    return segs

# ==============================================================================
# 폴더 단위 지능형 스캔 및 자동 연동/변환 총괄 함수
# ==============================================================================

def parse_braa0001_triplet(bt_path, bs_path, bg_path, code="AA0001"):
    with open(bt_path, "r", encoding="utf-16-le", errors="ignore") as f:
        bt_lines = [l.rstrip("\r\n") for l in f]
    with open(bs_path, "r", encoding="utf-16-le", errors="ignore") as f:
        bs_lines = [l.rstrip("\r\n") for l in f]
    with open(bg_path, "r", encoding="utf-16-le", errors="ignore") as f:
        bg_lines = [l.rstrip("\r\n") for l in f]

    bg_records = []
    in_body = False
    cur_sent = None
    cur_tree_lines = []
    for l in bg_lines:
        s = l.strip()
        if "<text><body>" in s or "<body>" in s:
            in_body = True
            continue
        if not in_body or "</body>" in s:
            continue
        if s.startswith(";"):
            m = re.match(r"^;\s*([A-Za-z0-9_]+)\s*(.*)$", s)
            if m:
                if cur_sent is not None:
                    bg_records.append((cur_sent, "\n".join(cur_tree_lines).strip()))
                    cur_tree_lines = []
                cur_sent = m.group(2).strip()
            continue
        if cur_sent is not None and s:
            cur_tree_lines.append(l)
    if cur_sent is not None:
        bg_records.append((cur_sent, "\n".join(cur_tree_lines).strip()))

    bs_map = {}
    for l in bs_lines:
        if "\t" in l and l.startswith("BS"):
            p = l.split("\t")
            if "-" in p[0]:
                bs_map[p[0].split("-")[1]] = p[2]

    eojeols = []
    cur_tag = "body"
    cur_date = ""
    cur_page = ""
    for l in bt_lines:
        s = l.strip()
        if s in ("<date>", "<page>", "<head>", "<p>"):
            cur_tag = s[1:-1]
        elif "\t" in l and l.startswith("BT"):
            p = l.split("\t")
            if "-" in p[0]:
                num = p[0].split("-")[1]
                if cur_tag == "date":
                    cur_date = p[1]
                elif cur_tag == "page":
                    cur_page = p[1]
                eojeols.append({
                    "num": num,
                    "bt_id": p[0],
                    "word": p[1],
                    "morph": p[2],
                    "sense": bs_map.get(num, ""),
                    "tag": cur_tag,
                    "page_info": f"{cur_date} p.{cur_page}".strip()
                })

    def norm(s):
        return re.sub(r"[\s`'\"“”‘’().,?!…\-]+", "", s)

    char_stream = []
    char_to_eojeol = []
    for idx, e in enumerate(eojeols):
        w = norm(e["word"])
        for ch in w:
            char_stream.append(ch)
            char_to_eojeol.append(idx)

    full_chars = "".join(char_stream)
    c_pos = 0
    segments = []
    for s_idx, (bg_sent, tree_str) in enumerate(bg_records, 1):
        t = norm(bg_sent)
        if not t:
            e_start = char_to_eojeol[min(c_pos, len(char_to_eojeol) - 1)] if char_to_eojeol else 0
            chunk = eojeols[e_start:e_start + 1]
        else:
            suffix = t[-min(len(t), 6):]
            expected_end = c_pos + len(t)
            search_from = max(c_pos + 1, expected_end - 25)
            search_to = min(len(full_chars), expected_end + 25)
            window = full_chars[search_from:search_to]
            best_end = None
            best_dist = 999999
            start_search = 0
            while True:
                idx = window.find(suffix, start_search)
                if idx == -1:
                    break
                cand_end = search_from + idx + len(suffix)
                dist = abs(cand_end - expected_end)
                if dist < best_dist:
                    best_dist = dist
                    best_end = cand_end
                start_search = idx + 1
            end_c = best_end if best_end is not None else min(len(full_chars), expected_end)
            e_start = char_to_eojeol[min(c_pos, len(char_to_eojeol) - 1)] if char_to_eojeol else 0
            e_end = (char_to_eojeol[min(max(c_pos, end_c - 1), len(char_to_eojeol) - 1)] + 1) if char_to_eojeol else 0
            chunk = eojeols[e_start:e_end]
            c_pos = end_c

        reconstructed_raw = " ".join(x["word"] for x in chunk) if chunk else bg_sent
        morph_flat = " | ".join(f"{x['word']}[{x['morph']}]" for x in chunk)
        sense_flat = " | ".join(f"{x['word']}[{x['sense']}]" for x in chunk)
        eojeol_json_str = json.dumps([
            {"id": x["bt_id"], "word": x["word"], "morph": x["morph"], "sense": x["sense"]}
            for x in chunk
        ], ensure_ascii=False)

        segments.append({
            "sent_id_str": f"BR{code}-S{s_idx:04d}",
            "part_type": chunk[0]["tag"] if chunk else "p",
            "lang": "kor",
            "page_info": chunk[0]["page_info"] if chunk else "",
            "text_raw": reconstructed_raw,
            "morph_flat": morph_flat,
            "sense_flat": sense_flat,
            "syntax_tree": tree_str,
            "eojeols_json": eojeol_json_str,
            "eojeol_cnt": len(chunk)
        })

    doc_info = {
        "doc_id": f"BR{code}_LINKED",
        "title": f"세종 3중 연동 말뭉치 (BR{code}) [형태·형태의미·구문]",
        "author": "국립국어원",
        "pub_year": "1993",
        "corpus_category": "다층 통합 말뭉치 (형태+의미+구문)",
        "annotation_layers": f"원문 + 형태소분석(BT{code}) + 어휘의미분석(BS{code}) + 구문분석(BG{code})"
    }
    file_records = [
        {
            "file_name": os.path.basename(bt_path),
            "file_size_bytes": os.path.getsize(bt_path),
            "original_encoding": "UTF-16LE (with BOM)",
            "text_format": "TEI.2 SGML/XML + TSV (3열)",
            "corpus_type": "형태 분석 말뭉치 (세종 문어)",
            "raw_notation_desc": "<p>, <head> 내 '어절ID \t 어절원문 \t 형태소분석' 2번째 열에 표기",
            "analysis_notation_desc": "3번째 열에 '형태소/품사 + ...' 표기",
            "imported_sentences": len(segments),
            "imported_eojeols": sum(s["eojeol_cnt"] for s in segments),
            "note": "동일 원문 기반 3중 통합 연동"
        },
        {
            "file_name": os.path.basename(bs_path),
            "file_size_bytes": os.path.getsize(bs_path),
            "original_encoding": "UTF-16LE (with BOM)",
            "text_format": "TEI.2 SGML/XML + TSV (3열)",
            "corpus_type": "형태의미 분석 말뭉치 (세종 문어)",
            "raw_notation_desc": "<p>, <head> 내 '어절ID \t 어절원문 \t 의미분석' 2번째 열에 표기",
            "analysis_notation_desc": "표준국어대사전 어깨번호 '__02/NNG' 표기",
            "imported_sentences": len(segments),
            "imported_eojeols": sum(s["eojeol_cnt"] for s in segments),
            "note": "동일 원문 기반 3중 통합 연동"
        },
        {
            "file_name": os.path.basename(bg_path),
            "file_size_bytes": os.path.getsize(bg_path),
            "original_encoding": "UTF-16LE (with BOM)",
            "text_format": "TEI.2 SGML/XML + S-식 구문 트리",
            "corpus_type": "구문 분석 말뭉치 (세종 트리뱅크)",
            "raw_notation_desc": "세미콜론('; ')으로 시작하는 주석 행에 전체 문장 원문 표기",
            "analysis_notation_desc": "소괄호 중첩 S-식 트리 구문 표지",
            "imported_sentences": len(segments),
            "imported_eojeols": sum(s["eojeol_cnt"] for s in segments),
            "note": "동일 원문 기반 3중 통합 연동"
        }
    ]
    return doc_info, file_records, segments

def parse_learner_xml(xml_path, doc_id_prefix):
    tree = ET.parse(xml_path)
    root = tree.getroot()
    header = root.find("Header")
    theme = header.findtext("AssignmentTheme", "") if header is not None else ""
    nat = header.findtext(".//Nationality", "") if header is not None else ""
    grade = header.findtext(".//DataGrade", "") if header is not None else ""

    segments = []
    s_idx = 0
    for sent_el in root.findall(".//SENTENCE"):
        s_idx += 1
        raw_s = (sent_el.findtext("s") or "").strip()
        words_data = []
        morph_flat_parts = []
        for w_idx, w_el in enumerate(sent_el.findall(".//word"), 1):
            w_form = (w_el.findtext("w") or "").strip()
            m_items = []
            for m_el in w_el.findall("morph"):
                m_form = (m_el.text or "").strip()
                m_pos = m_el.attrib.get("pos", "")
                m_items.append(f"{m_form}/{m_pos}")
            m_str = " + ".join(m_items)
            morph_flat_parts.append(f"{w_form}[{m_str}]")
            words_data.append({
                "id": f"W{w_idx}",
                "word": w_form,
                "morph": m_str,
                "sense": ""
            })
        segments.append({
            "sent_id_str": f"{doc_id_prefix}-S{s_idx:03d}",
            "part_type": "main",
            "lang": "kor",
            "page_info": f"{nat}/{grade}",
            "text_raw": raw_s,
            "morph_flat": " | ".join(morph_flat_parts),
            "sense_flat": "",
            "syntax_tree": "",
            "eojeols_json": json.dumps(words_data, ensure_ascii=False),
            "eojeol_cnt": len(words_data)
        })
    return theme, nat, grade, segments

def build_corpus_from_folder(folder_path: str, db_path: str = DEFAULT_DB_PATH, progress_callback=None):
    """
    사용자가 지정한 폴더를 분석하여 최적의 데이터베이스를 구축합니다.
    """
    if not os.path.exists(folder_path):
        raise FileNotFoundError(f"지정한 폴더가 존재하지 않습니다: {folder_path}")

    folder = Path(folder_path)
    valid_exts = {".txt", ".xml", ".json", ".sgml", ".tei", ".csv", ".tsv"}
    all_files = [
        f for f in folder.rglob("*")
        if f.is_file() and f.suffix.lower() in valid_exts and not f.name.startswith(".")
    ]
    if not all_files:
        ignore_exts = {".corpdb", ".db", ".sqlite", ".sqlite3", ".zip", ".exe", ".dll", ".ttf", ".otf", ".png", ".jpg", ".pdf"}
        all_files = [
            f for f in folder.rglob("*")
            if f.is_file() and not f.name.startswith(".") and f.suffix.lower() not in ignore_exts
        ]
    if not all_files:
        raise ValueError(f"폴더 내에 파일이 없습니다: {folder_path}")

    conn = sqlite3.connect(db_path)
    init_db(conn)

    report = {
        "folder": str(folder_path),
        "db_path": str(db_path),
        "total_files_scanned": len(all_files),
        "documents_created": 0,
        "sentences_imported": 0,
        "eojeols_imported": 0,
        "items": []
    }

    # 1. 파일 이름 패턴 기반 세종 3중 연동 감지 (BTAA + BSAA + BGAA)
    stem_map = {}
    for f in all_files:
        name = f.name
        # BT*, BS*, BG* 패턴
        if re.match(r"^(BT|BS|BG)([A-Z0-9]+)\.txt$", name, re.I):
            m = re.match(r"^(BT|BS|BG)([A-Z0-9]+)\.txt$", name, re.I)
            kind, code = m.group(1).upper(), m.group(2).upper()
            stem_map.setdefault(code, {})[kind] = f

    processed_files = set()

    for code, kmap in stem_map.items():
        if "BT" in kmap and "BS" in kmap and "BG" in kmap:
            # 세종 3중 연동 처리!
            if progress_callback:
                progress_callback(f"세종 3중 연동 말뭉치 감지 (코드 {code}): BTAA+BSAA+BGAA 처리 중...")
            
            # self-contained
            doc_info, file_records, segments = parse_braa0001_triplet(
                str(kmap["BT"]), str(kmap["BS"]), str(kmap["BG"]), code
            )
            s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_records, segments)
            for k in ("BT", "BS", "BG"):
                processed_files.add(kmap[k].name)
            report["documents_created"] += 1
            report["sentences_imported"] += s_cnt
            report["eojeols_imported"] += e_cnt
            report["items"].append({
                "type": "세종 3중 연동",
                "doc_id": doc_info["doc_id"],
                "files": [kmap["BT"].name, kmap["BS"].name, kmap["BG"].name],
                "sentences": s_cnt
            })

    # 2. 학습자 말뭉치 연동 감지 (sample_*.txt + sample_*.xml)
    learner_pairs = {}
    for f in all_files:
        if f.name in processed_files:
            continue
        m = re.match(r"^(.*)\.(txt|xml)$", f.name, re.I)
        if m:
            base, ext = m.group(1), m.group(2).lower()
            learner_pairs.setdefault(base, {})[ext] = f

    for base, extmap in learner_pairs.items():
        if "txt" in extmap and "xml" in extmap:
            sample_txt, enc_t, _ = read_sample_text(str(extmap["txt"]), 512)
            sample_xml, enc_x, _ = read_sample_text(str(extmap["xml"]), 512)
            if "<Korean_Learners_Corpus>" in sample_xml or "<AssignmentTheme>" in sample_xml:
                if progress_callback:
                    progress_callback(f"학습자 연동 말뭉치 감지: {base}.txt + {base}.xml 처리 중...")
                # self-contained
                theme, nat, grade, segs = parse_learner_xml(str(extmap["xml"]), base.upper().replace("-", "_"))
                doc_info = {
                    "doc_id": f"{base.upper().replace('-', '_')}_LINKED",
                    "title": f"한국어 학습자 말뭉치 ({theme or base}) [원시+형태 연동]",
                    "author": f"학습자({nat or '미상'}, {grade or '급수미상'})",
                    "pub_year": "2015",
                    "corpus_category": "다층 통합 말뭉치 (학습자 원시+형태)",
                    "annotation_layers": f"원시텍스트({extmap['txt'].name}) + 형태소주석XML({extmap['xml'].name})"
                }
                file_recs = [
                    {
                        "file_name": extmap["txt"].name,
                        "file_size_bytes": os.path.getsize(str(extmap["txt"])),
                        "original_encoding": enc_t,
                        "text_format": "순수 텍스트 (.txt)",
                        "corpus_type": "원시 말뭉치 (학습자 작문)",
                        "raw_notation_desc": "줄바꿈 단위 문장 평문",
                        "analysis_notation_desc": f"{extmap['xml'].name} 형태분석과 1:1 연동",
                        "note": "원시+형태 연동"
                    },
                    {
                        "file_name": extmap["xml"].name,
                        "file_size_bytes": os.path.getsize(str(extmap["xml"])),
                        "original_encoding": enc_x,
                        "text_format": "XML (<Korean_Learners_Corpus>)",
                        "corpus_type": "형태 분석 말뭉치 (학습자 주석 XML)",
                        "raw_notation_desc": "<SENTENCE><s> 및 <word><w> 태그",
                        "analysis_notation_desc": "<morph pos='...'> 태그 표기",
                        "note": "원시+형태 연동"
                    }
                ]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                processed_files.add(extmap["txt"].name)
                processed_files.add(extmap["xml"].name)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({
                    "type": "학습자 원시+형태 연동",
                    "doc_id": doc_info["doc_id"],
                    "files": [extmap["txt"].name, extmap["xml"].name],
                    "sentences": s_cnt
                })

    # 3. 개별 파일 내용 기반 유형 자동 판별
    for f in all_files:
        if f.name in processed_files:
            continue
        sample_str, enc, enc_label = read_sample_text(str(f), 8192)
        f_size = os.path.getsize(str(f))
        fname = f.name
        fext = f.suffix.lower()

        # (A) XML 파일 판별
        if fext == ".xml":
            # A-1. 옛한글 사전/자석류 (<c><g><m><s> 또는 훈몽자회)
            if "<c><g>" in sample_str and "<m>" in sample_str and "<s>" in sample_str:
                if progress_callback:
                    progress_callback(f"옛한글 사전/자석류 말뭉치 감지: {fname} 처리 중...")
                segs = parse_dictionary_xml(str(f), enc)
                doc_info = {
                    "doc_id": f"OLD_DICT_{f.stem.upper().replace(' ', '_')}",
                    "title": f"{f.stem} (옛한글 사전·자석류)",
                    "author": "미상",
                    "pub_year": "조선시대",
                    "corpus_category": "옛한글 한자 자석(사전형 훈·음 대역) 말뭉치",
                    "annotation_layers": "한자(<g>) + 훈(<m>) + 음(<s>) 구조화"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "XML (<c><g><m><s>)",
                    "corpus_type": "옛한글 사전/자석 말뭉치",
                    "raw_notation_desc": "<c><g>한자</g><m>훈</m><s>음</s></c>",
                    "analysis_notation_desc": "한자, 옛한글 훈, 옛한글 음 구조화 분리"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": "옛한글 사전류", "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

            # A-2. 옛한글 언해문/대역/문헌류 (<sent ...> ... </sent>)
            elif "<sent" in sample_str and "</sent>" in sample_str:
                if progress_callback:
                    progress_callback(f"옛한글 문헌/언해문 말뭉치 감지: {fname} 처리 중...")
                segs = parse_sent_xml(str(f), enc)
                doc_info = {
                    "doc_id": f"OLD_{f.stem.upper().replace(' ', '_')}",
                    "title": f"{f.stem} (옛한글 문헌·언해본)",
                    "author": "미상",
                    "pub_year": "조선시대",
                    "corpus_category": "옛한글 문헌·언해·대역 마크업 말뭉치",
                    "annotation_layers": "원문(한양PUA+첫가끝) + 장차(page)/언어(lang)/문장번호(n) 마크업"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "XML (<sent>)",
                    "corpus_type": "옛한글 문헌 마크업 말뭉치",
                    "raw_notation_desc": "<sent> 태그 내 원문 (장넘김 '^')",
                    "analysis_notation_desc": "type, lang, page, n 속성 마크업"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": "옛한글 문헌/언해문", "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

            # A-3. 단독 학습자 형태 분석 XML
            elif "<Korean_Learners_Corpus>" in sample_str or "<SENTENCE>" in sample_str:
                if progress_callback:
                    progress_callback(f"학습자 형태 주석 XML 감지: {fname} 처리 중...")
                # self-contained
                theme, nat, grade, segs = parse_learner_xml(str(f), f.stem.upper().replace("-", "_"))
                doc_info = {
                    "doc_id": f"LEARNER_{f.stem.upper().replace('-', '_')}",
                    "title": f"한국어 학습자 말뭉치 ({theme or f.stem})",
                    "author": f"학습자({nat or '미상'}, {grade or '급수미상'})",
                    "pub_year": "2015",
                    "corpus_category": "형태 분석 말뭉치 (학습자 XML)",
                    "annotation_layers": "원문 + 형태소주석(<morph pos>)"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "XML (<Korean_Learners_Corpus>)",
                    "corpus_type": "형태 분석 말뭉치 (학습자 주석 XML)",
                    "raw_notation_desc": "<SENTENCE><s> 및 <word><w> 태그",
                    "analysis_notation_desc": "<morph pos='...'> 태그 표기"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": "학습자 형태 XML", "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

        # (B) JSON 파일 판별 (모두의 말뭉치 표준)
        elif fext == ".json":
            if progress_callback:
                progress_callback(f"모두의 말뭉치 JSON 감지: {fname} 처리 중...")
            segs, has_morph = parse_nikl_json(str(f), enc)
            cat = "형태 분석 말뭉치 (모두의 말뭉치 JSON)" if has_morph else "원시 말뭉치 (모두의 말뭉치 JSON)"
            doc_info = {
                "doc_id": f"NIKL_{f.stem.upper().replace('-', '_')}",
                "title": f"국립국어원 모두의 말뭉치 ({f.stem})",
                "author": "국립국어원",
                "pub_year": "2019",
                "corpus_category": cat,
                "annotation_layers": "원문 + 형태소 분석(morpheme)" if has_morph else "문단 원문 + 문서 메타데이터"
            }
            file_recs = [{
                "file_name": fname,
                "file_size_bytes": f_size,
                "original_encoding": enc_label,
                "text_format": "JSON (국립국어원 표준)",
                "corpus_type": cat,
                "raw_notation_desc": "sentence[].form 또는 paragraph[].form 표기",
                "analysis_notation_desc": "morpheme[] 분리 표기" if has_morph else "메타데이터 객체 표기"
            }]
            s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
            report["documents_created"] += 1
            report["sentences_imported"] += s_cnt
            report["eojeols_imported"] += e_cnt
            report["items"].append({"type": cat, "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

        # (C) TXT 파일 판별
        elif fext == ".txt":
            # C-1. 세종 구어 형태 분석 (5CT* 스타일: 행ID \t 어절 \t 분석)
            if "5CT_" in fname or ("\t" in sample_str and re.search(r"/[A-Z]{2,3}\+", sample_str)):
                if progress_callback:
                    progress_callback(f"세종 구어 형태 분석 말뭉치 감지: {fname} 처리 중...")
                segs = parse_sejong_spoken_morph(str(f), enc)
                doc_info = {
                    "doc_id": f"SEJONG_MORPH_{f.stem.upper().replace('-', '_')}",
                    "title": f"{f.stem} (세종 구어 형태소 분석)",
                    "author": "국립국어원",
                    "pub_year": "2001",
                    "corpus_category": "형태 분석 말뭉치 (세종 구어)",
                    "annotation_layers": "구어 발화 원문 + 어절별 형태소 분석"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "TEI.2 SGML/XML + TSV",
                    "corpus_type": "형태 분석 말뭉치 (세종 구어)",
                    "raw_notation_desc": "행ID \\t 어절 \\t 형태소분석",
                    "analysis_notation_desc": "공백 없는 '+' 결합 형태소/품사"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": "세종 구어 형태소", "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

            # C-2. 세종 구어 전사 원시 (4CM* 스타일: <u who> <s n>)
            elif "<u who=" in sample_str and "<s n=" in sample_str:
                if progress_callback:
                    progress_callback(f"세종 구어 전사 원시 말뭉치 감지: {fname} 처리 중...")
                segs = parse_sejong_tei_spoken_raw(str(f), enc)
                doc_info = {
                    "doc_id": f"SEJONG_SPOKEN_{f.stem.upper().replace('-', '_')}",
                    "title": f"{f.stem} (세종 구어 전사 원시)",
                    "author": "국립국어원",
                    "pub_year": "2001",
                    "corpus_category": "원시 말뭉치 (세종 구어 전사)",
                    "annotation_layers": "구어 발화 원문 + 화자/인라인 마크업"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "TEI.2 SGML/XML",
                    "corpus_type": "원시 말뭉치 (세종 구어 전사)",
                    "raw_notation_desc": "<u who='P1'><s n='00001'>발화문</s></u>",
                    "analysis_notation_desc": "억양/웃음/발음 인라인 마크업"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": "세종 구어 전사", "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

            # C-3. 세종 문어 TEI 원시 (2BA* 스타일: <date>, <page>, <head>, <p>)
            elif "<tei.2>" in sample_str and ("<head>" in sample_str or "<p>" in sample_str):
                if progress_callback:
                    progress_callback(f"세종 문어 원시 TEI 말뭉치 감지: {fname} 처리 중...")
                segs = parse_sejong_tei_written(str(f), enc)
                doc_info = {
                    "doc_id": f"SEJONG_RAW_{f.stem.upper().replace('-', '_')}",
                    "title": f"{f.stem} (세종 문어 원시)",
                    "author": "국립국어원",
                    "pub_year": "1990년대",
                    "corpus_category": "원시 말뭉치 (세종 문어 TEI)",
                    "annotation_layers": "원문 + 날짜/면수/제목/문단 마크업"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "TEI.2 SGML/XML",
                    "corpus_type": "원시 말뭉치 (세종 문어)",
                    "raw_notation_desc": "<head>제목</head>, <p>문단</p>",
                    "analysis_notation_desc": "출전 일자 및 지면 표기"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": "세종 문어 원시", "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

            # C-4. 일반 평문 및 대본/희곡 텍스트
            else:
                if progress_callback:
                    progress_callback(f"일반 텍스트/대본 말뭉치 감지: {fname} 처리 중...")
                segs = parse_plain_or_script(str(f), enc)
                is_script = "#" in sample_str
                cat = "원시 말뭉치 (방송/희곡 대본)" if is_script else "원시 말뭉치 (일반 평문)"
                doc_info = {
                    "doc_id": f"RAW_{f.stem.upper().replace(' ', '_').replace('-', '_')}",
                    "title": f"{f.stem} ({cat})",
                    "author": "미상",
                    "pub_year": "현대",
                    "corpus_category": cat,
                    "annotation_layers": "원문 텍스트"
                }
                file_recs = [{
                    "file_name": fname,
                    "file_size_bytes": f_size,
                    "original_encoding": enc_label,
                    "text_format": "평문 텍스트 (.txt)",
                    "corpus_type": cat,
                    "raw_notation_desc": "줄바꿈 단위 원문 문장 표기",
                    "analysis_notation_desc": "별도 형태소 분석 없음"
                }]
                s_cnt, e_cnt = save_document_and_segments(conn, doc_info, file_recs, segs)
                report["documents_created"] += 1
                report["sentences_imported"] += s_cnt
                report["eojeols_imported"] += e_cnt
                report["items"].append({"type": cat, "doc_id": doc_info["doc_id"], "files": [fname], "sentences": s_cnt})

    conn.close()
    if progress_callback:
        progress_callback(f"데이터베이스 구축 완료! 총 {report['documents_created']}개 문서, {report['sentences_imported']:,}문장 생성됨.")
    return report

if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="폴더 지정 말뭉치 데이터베이스 자동 구축기")
    parser.add_argument("--folder", type=str, default=os.path.join(WORKSPACE, "sample"), help="변환할 말뭉치 파일들이 있는 폴더 경로")
    parser.add_argument("--db", type=str, default=DEFAULT_DB_PATH, help="대상 SQLite DB 파일 경로")
    args = parser.parse_args()

    print(f"=== 폴더 말뭉치 DB 변환 시작: {args.folder} -> {args.db} ===")
    res = build_corpus_from_folder(args.folder, args.db, lambda msg: print(f"[*] {msg}"))
    print(json.dumps(res, ensure_ascii=False, indent=2))
