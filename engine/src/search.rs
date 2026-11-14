use crate::models::{binary_response, json_response, ApiResponse, AppState, SearchHit, SearchResponse};
use crate::normalizer::{find_bracket_query_match, match_tokens_at, BracketQueryToken};
use regex::Regex;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

/// Extract KWIC Left Context, Node, and Right Context by Eojeol (word) count!
/// Default `context_words` is 5 (좌우 5어절).
pub(crate) fn extract_kwic_by_word_count(
    text: &str,
    start_char: usize,
    end_char: usize,
    context_words: usize,
) -> (String, String, String, usize, usize, bool, bool) {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let s = start_char.min(total);
    let e = end_char.min(total).max(s);

    // 핵심어(Node)가 한 음절에 그치지 않고 검색된 대상을 포함한 전체 어절(Eojeol) 단위로 나오도록 확장
    let mut node_s = s;
    if node_s < total && !chars[node_s].is_whitespace() {
        while node_s > 0 && !chars[node_s - 1].is_whitespace() {
            node_s -= 1;
        }
    }
    let mut node_e = e;
    if node_e > node_s {
        if node_e <= total && !chars[node_e - 1].is_whitespace() {
            while node_e < total && !chars[node_e].is_whitespace() {
                node_e += 1;
            }
        }
    } else {
        node_e = node_s;
    }

    let node: String = chars[node_s..node_e].iter().collect();

    if context_words >= 999 {
        let left: String = chars[0..node_s].iter().collect();
        let right: String = chars[node_e..total].iter().collect();
        return (left, node, right, 999, 999, false, false);
    }

    // Left Context: node_s 직전부터 context_words 개수만큼의 앞선 어절 카운트
    let mut left_idx = node_s;
    let mut words_counted = 0usize;
    while left_idx > 0 && words_counted < context_words {
        // 단어 앞 공백 건너뛰기
        let mut ws_pos = left_idx;
        while ws_pos > 0 && chars[ws_pos - 1].is_whitespace() {
            ws_pos -= 1;
        }
        if ws_pos == 0 {
            left_idx = 0;
            break;
        }
        // 단어 문자 건너뛰기
        while ws_pos > 0 && !chars[ws_pos - 1].is_whitespace() {
            ws_pos -= 1;
        }
        left_idx = ws_pos;
        words_counted += 1;
    }

    let left: String = chars[left_idx..node_s].iter().collect();
    let left_truncated = left_idx > 0;

    // Right Context: node_e 직후부터 context_words 개수만큼의 뒤선 어절 카운트
    let mut right_idx = node_e;
    let mut r_words_counted = 0usize;
    while right_idx < total && r_words_counted < context_words {
        let mut ws_pos = right_idx;
        while ws_pos < total && chars[ws_pos].is_whitespace() {
            ws_pos += 1;
        }
        if ws_pos == total {
            right_idx = total;
            break;
        }
        while ws_pos < total && !chars[ws_pos].is_whitespace() {
            ws_pos += 1;
        }
        right_idx = ws_pos;
        r_words_counted += 1;
    }

    let right: String = chars[node_e..right_idx].iter().collect();
    let right_truncated = right_idx < total;

    (
        left,
        node,
        right,
        words_counted,
        r_words_counted,
        left_truncated,
        right_truncated,
    )
}

pub(crate) fn pad_kwic_context(
    conn: &Connection,
    doc_id: &str,
    seq_no: i64,
    mut left: String,
    node: String,
    mut right: String,
    words_counted: usize,
    r_words_counted: usize,
    context_words: usize,
    left_truncated: bool,
    right_truncated: bool,
) -> (String, String, String) {
    if context_words >= 999 {
        return (left, node, right);
    }

    // 1) Left Context padding across previous segments
    let mut left_has_more = left_truncated;
    if words_counted < context_words {
        let mut needed = context_words - words_counted;
        let mut prev_words = Vec::new();
        if let Ok(mut stmt_prev) = conn.prepare_cached(
            "SELECT text_unicode FROM segments WHERE doc_id = ? AND seq_no < ? ORDER BY seq_no DESC LIMIT 3",
        ) {
            if let Ok(mut rows) = stmt_prev.query(params![doc_id, seq_no]) {
                while let Ok(Some(row)) = rows.next() {
                    let prev_text: String = row.get(0).unwrap_or_default();
                    let words: Vec<&str> = prev_text.split_whitespace().collect();
                    if words.is_empty() {
                        continue;
                    }
                    if words.len() <= needed {
                        needed -= words.len();
                        for w in words.into_iter().rev() {
                            prev_words.push(w.to_string());
                        }
                    } else {
                        let take_start = words.len() - needed;
                        for w in words[take_start..].iter().rev() {
                            prev_words.push((*w).to_string());
                        }
                        left_has_more = true;
                        break;
                    }
                    if needed == 0 {
                        break;
                    }
                }
            }
        }
        if !prev_words.is_empty() {
            prev_words.reverse();
            let prepended = prev_words.join(" ");
            let trimmed = left.trim_start();
            left = if trimmed.is_empty() {
                prepended
            } else {
                format!("{} {}", prepended, trimmed)
            };
        }
    }
    if left_has_more {
        left = format!("…{}", left.trim_start());
    }

    // 2) Right Context padding across succeeding segments
    let mut right_has_more = right_truncated;
    if r_words_counted < context_words {
        let mut needed = context_words - r_words_counted;
        let mut next_words = Vec::new();
        if let Ok(mut stmt_next) = conn.prepare_cached(
            "SELECT text_unicode FROM segments WHERE doc_id = ? AND seq_no > ? ORDER BY seq_no ASC LIMIT 3",
        ) {
            if let Ok(mut rows) = stmt_next.query(params![doc_id, seq_no]) {
                while let Ok(Some(row)) = rows.next() {
                    let next_text: String = row.get(0).unwrap_or_default();
                    let words: Vec<&str> = next_text.split_whitespace().collect();
                    if words.is_empty() {
                        continue;
                    }
                    if words.len() <= needed {
                        needed -= words.len();
                        for w in words {
                            next_words.push(w.to_string());
                        }
                    } else {
                        for w in &words[..needed] {
                            next_words.push((*w).to_string());
                        }
                        right_has_more = true;
                        break;
                    }
                    if needed == 0 {
                        break;
                    }
                }
            }
        }
        if !next_words.is_empty() {
            let appended = next_words.join(" ");
            let trimmed = right.trim_end();
            right = if trimmed.is_empty() {
                appended
            } else {
                format!("{} {}", trimmed, appended)
            };
        }
    }
    if right_has_more {
        right = format!("{}…", right.trim_end());
    }

    (left, node, right)
}

#[allow(dead_code)]
pub(crate) fn find_char_subslice(haystack: &[char], needle: &[char]) -> Option<(usize, usize)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let first = needle[0];
    let max_start = haystack.len() - needle.len();
    for i in 0..=max_start {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            return Some((i, i + needle.len()));
        }
    }
    None
}

pub(crate) fn find_all_char_subslices(haystack: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    if needle.is_empty() || needle.len() > haystack.len() {
        return matches;
    }
    let first = needle[0];
    let max_start = haystack.len() - needle.len();
    for i in 0..=max_start {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            matches.push((i, i + needle.len()));
        }
    }
    matches
}

pub(crate) fn is_jongseong_jamo(c: char) -> bool {
    matches!(c, '\u{11A8}'..='\u{11FF}' | '\u{D7CB}'..='\u{D7FB}')
}

pub(crate) fn is_choseong_only_query(q_norm: &[char]) -> bool {
    !q_norm.is_empty()
        && q_norm.len() >= 2
        && q_norm.iter().all(|&c| {
            let cp = c as u32;
            (0x1100..=0x115E).contains(&cp) || (0xA960..=0xA97C).contains(&cp)
        })
}

pub(crate) fn handle_search_cancel(state: &AppState) -> ApiResponse {
    let cur = state.latest_search_seq.load(Ordering::Relaxed);
    state.cancel_search_seq.store(cur, Ordering::Relaxed);
    json_response(&serde_json::json!({ "ok": true, "cancelled_seq": cur }), 200)
}


/// Expands `[초/중/종]` bracket expressions into standard Regular Expression character classes
/// so that regex and jamo bracket queries can be executed seamlessly together.
pub(crate) fn expand_bracket_syntax_to_regex(query: &str) -> String {
    if !query.contains('[') || !query.contains('/') || !query.contains(']') {
        return query.to_string();
    }
    const CHO_LIST: [char; 19] = [
        'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
    ];
    const JUNG_LIST: [char; 21] = [
        'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
    ];
    const JONG_LIST: [&str; 28] = [
        "", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ", "ㄿ", "ㅀ", "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ",
    ];

    let chars: Vec<char> = query.chars().collect();
    let mut out = String::with_capacity(query.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut end_br = i + 1;
            while end_br < chars.len() && chars[end_br] != ']' {
                end_br += 1;
            }
            if end_br < chars.len() && chars[end_br] == ']' {
                let inner: String = chars[i + 1..end_br].iter().collect();
                if inner.contains('/') {
                    let parts: Vec<&str> = inner.split('/').collect();
                    let cho_part = parts[0].trim();
                    let jung_part = parts.get(1).map(|s| s.trim()).unwrap_or("");
                    let jong_part = parts.get(2).map(|s| s.trim()).unwrap_or("0");

                    // Determine Cho indices
                    let cho_indices: Vec<usize> = if cho_part == "1" || cho_part == "?" || cho_part == "*" {
                        (0..19).collect()
                    } else {
                        CHO_LIST.iter().enumerate().filter(|(_, &c)| cho_part.contains(c)).map(|(idx, _)| idx).collect()
                    };

                    // Determine Jung indices
                    let jung_indices: Vec<usize> = if jung_part == "1" || jung_part == "?" || jung_part == "*" {
                        (0..21).collect()
                    } else {
                        JUNG_LIST.iter().enumerate().filter(|(_, &c)| jung_part.contains(c)).map(|(idx, _)| idx).collect()
                    };

                    // Determine Jong indices
                    let jong_indices: Vec<usize> = if jong_part == "0" {
                        vec![0]
                    } else if jong_part == "1" || jong_part == "?" {
                        (1..28).collect()
                    } else if jong_part == "01" || jong_part == "*" {
                        (0..28).collect()
                    } else {
                        JONG_LIST.iter().enumerate().filter(|(_, &s)| !s.is_empty() && jong_part.contains(s)).map(|(idx, _)| idx).collect()
                    };

                    let mut syllables = Vec::new();
                    for &l in &cho_indices {
                        for &v in &jung_indices {
                            for &t in &jong_indices {
                                let code = 0xAC00 + (l * 21 + v) * 28 + t;
                                if let Some(ch) = char::from_u32(code as u32) {
                                    syllables.push(ch);
                                }
                            }
                        }
                    }

                    if !syllables.is_empty() {
                        if syllables.len() == 1 {
                            out.push(syllables[0]);
                        } else if syllables.len() > 100 {
                            // If very large set (e.g. [1/1/0]), compress or emit bracket class
                            out.push('[');
                            for ch in syllables {
                                out.push(ch);
                            }
                            out.push(']');
                        } else {
                            out.push('[');
                            for ch in syllables {
                                out.push(ch);
                            }
                            out.push(']');
                        }
                        i = end_br + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub(crate) fn check_word_boundary(
    display_chars: &[char],
    orig_start: usize,
    orig_end: usize,
    has_leading_space: bool,
    has_trailing_space: bool,
) -> bool {
    if has_leading_space && orig_start > 0 {
        if let Some(&prev_c) = display_chars.get(orig_start - 1) {
            if !prev_c.is_whitespace() && !prev_c.is_ascii_punctuation() {
                return false;
            }
        }
    }
    if has_trailing_space && orig_end < display_chars.len() {
        if let Some(&next_c) = display_chars.get(orig_end) {
            if !next_c.is_whitespace() && !next_c.is_ascii_punctuation() {
                return false;
            }
        }
    }
    true
}

/// Checks if the query contains regular expression metacharacters outside `[초/중/종]` Jamo brackets.
pub(crate) fn has_regex_syntax(query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    // Remove [초/중/종] jamo brackets first
    let chars: Vec<char> = query.chars().collect();
    let mut stripped = String::with_capacity(query.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut end_br = i + 1;
            while end_br < chars.len() && chars[end_br] != ']' {
                end_br += 1;
            }
            if end_br < chars.len() && chars[end_br] == ']' {
                let inner: String = chars[i + 1..end_br].iter().collect();
                if inner.contains('/') {
                    i = end_br + 1;
                    continue;
                }
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    stripped.contains(".{")
        || stripped.contains(".*")
        || stripped.contains(".+")
        || stripped.contains(".?")
        || stripped.contains("\\d")
        || stripped.contains("\\s")
        || stripped.contains("\\S")
        || stripped.contains("\\w")
        || stripped.contains("\\b")
        || stripped.contains('^')
        || stripped.contains('$')
        || stripped.contains('|')
        || (stripped.contains('{') && stripped.contains('}'))
        || (stripped.contains('[') && stripped.contains(']'))
        || (stripped.contains('(') && stripped.contains(')'))
}

/// Determines whether a query requires matching at the start of an eojeol (`어절 시작부분 매칭`)
/// when in `eojeol` or `sentence` search unit mode.
pub(crate) fn query_requires_eojeol_start(core_query: &str) -> bool {
    let q = core_query.trim();
    if q.is_empty() {
        return false;
    }
    if q.starts_with('^') {
        return true;
    }
    // Explicit leading wildcards or tag/sense delimiters allow mid-eojeol matching
    if q.starts_with(".*")
        || q.starts_with(".+")
        || q.starts_with(".?")
        || q.starts_with('*')
        || q.starts_with('?')
        || q.starts_with("\\S*")
        || q.starts_with("\\w*")
        || q.starts_with('/')
        || q.starts_with('_')
        || q.starts_with('+')
        || q.starts_with('-')
        || q.starts_with(')')
        || q.starts_with('(')
    {
        return false;
    }
    // Pure POS / syntax category tag (e.g., NNG, VV, ETM, NP_SBJ)
    if q.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '/' || c == '+')
    {
        return false;
    }
    true
}

pub(crate) fn is_leading_quote_or_bracket(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '“' | '”' | '‘' | '’' | '(' | '<' | '{' | '「' | '『' | '《' | '〈' | '【' | '〔'
    )
}

pub(crate) fn strip_leading_surface_quotes(s: &str) -> (&str, usize) {
    let mut byte_idx = 0;
    let mut skipped_chars = 0;
    for (b_i, ch) in s.char_indices() {
        if is_leading_quote_or_bracket(ch) {
            byte_idx = b_i + ch.len_utf8();
            skipped_chars += 1;
        } else {
            break;
        }
    }
    (&s[byte_idx..], skipped_chars)
}

pub(crate) fn strip_leading_morph_quotes(s: &str) -> &str {
    let mut cur = s.trim_start();
    loop {
        if let Some(plus_idx) = cur.find('+') {
            let first_m = cur[..plus_idx].trim();
            if first_m.ends_with("/SS") || first_m.ends_with("/SP") || first_m.ends_with("/SO") {
                cur = cur[plus_idx + 1..].trim_start();
                continue;
            }
        }
        break;
    }
    cur
}

pub(crate) fn split_eojeol_word_and_analysis(ea: &str) -> (&str, &str) {
    if let Some(open_idx) = ea.find('[') {
        if ea.ends_with(']') && open_idx + 1 < ea.len() {
            return (&ea[..open_idx], &ea[open_idx + 1..ea.len() - 1]);
        }
    }
    (ea, ea)
}

pub(crate) fn compute_display_word_spans(display_text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut cur_char = 0usize;
    let mut in_word = false;
    let mut word_start_byte = 0usize;
    let mut word_start_char = 0usize;

    for (b_idx, ch) in display_text.char_indices() {
        if ch.is_whitespace() {
            if in_word {
                spans.push((
                    word_start_char,
                    cur_char,
                    &display_text[word_start_byte..b_idx],
                ));
                in_word = false;
            }
        } else if !in_word {
            in_word = true;
            word_start_byte = b_idx;
            word_start_char = cur_char;
        }
        cur_char += 1;
    }
    if in_word {
        spans.push((
            word_start_char,
            cur_char,
            &display_text[word_start_byte..],
        ));
    }
    spans
}

pub(crate) fn find_eojeol_span_in_display(
    display_text: &str,
    word_spans: &[(usize, usize, &str)],
    start_eojeol_idx: usize,
    end_eojeol_idx: usize,
    first_ea_word: &str,
) -> (usize, usize) {
    if !word_spans.is_empty() {
        let s_idx = start_eojeol_idx.min(word_spans.len() - 1);
        let e_idx = end_eojeol_idx.max(s_idx).min(word_spans.len() - 1);
        let (_, _, w_at_s) = word_spans[s_idx];
        if first_ea_word.is_empty()
            || w_at_s == first_ea_word
            || w_at_s.contains(first_ea_word)
            || first_ea_word.contains(w_at_s)
        {
            return (word_spans[s_idx].0, word_spans[e_idx].1);
        }
        // Search nearby word_spans if whitespace splitting shifted indices
        for (idx, &(sc, _, w_str)) in word_spans.iter().enumerate() {
            if !first_ea_word.is_empty()
                && (w_str == first_ea_word
                    || w_str.contains(first_ea_word)
                    || first_ea_word.contains(w_str))
            {
                let span_len = end_eojeol_idx.saturating_sub(start_eojeol_idx);
                let target_e = (idx + span_len).min(word_spans.len() - 1);
                return (sc, word_spans[target_e].1);
            }
        }
        return (word_spans[s_idx].0, word_spans[e_idx].1);
    }
    if !first_ea_word.is_empty() {
        if let Some(byte_pos) = display_text.find(first_ea_word) {
            let sc = display_text[..byte_pos].chars().count();
            let ec = sc + first_ea_word.chars().count();
            return (sc, ec);
        }
    }
    (0, display_text.chars().count().min(6).max(1))
}

pub(crate) struct SearchQueryContext<'a> {
    pub(crate) core_query: &'a str,
    pub(crate) search_unit: &'a str, // "eojeol" | "sentence" | "text"
    pub(crate) req_eojeol_start: bool,
    pub(crate) has_leading_space: bool,
    pub(crate) has_trailing_space: bool,
    pub(crate) use_surface: bool,
    pub(crate) use_morph: bool,
    pub(crate) use_sense: bool,
    pub(crate) use_syntax: bool,
    pub(crate) use_text: bool,
    pub(crate) use_jamo: bool,
    pub(crate) use_regex: bool,
    pub(crate) norm_query_chars: &'a [char],
    pub(crate) norm_morph_query: &'a str,
    pub(crate) bracket_tokens: Option<&'a [BracketQueryToken]>,
    pub(crate) compiled_regex: Option<&'a Regex>,
    pub(crate) anchored_regex: Option<&'a Regex>,
}

pub(crate) fn match_analysis_layer(
    state: &AppState,
    qctx: &SearchQueryContext,
    display_text: &str,
    flat_str: &str,
    layer_label: &'static str,
    layer_jamo_label: &'static str,
    layer_regex_label: &'static str,
) -> Option<(usize, usize, &'static str)> {
    if flat_str.is_empty() {
        return None;
    }

    let req_start = qctx.req_eojeol_start && qctx.search_unit != "text";
    let cq_clean = if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
        Some(qctx.core_query.replace(" + ", "+"))
    } else {
        None
    };

    // 1. Check single-eojeol matches first (lazy allocation: only build what's needed)
    let mut ea_count = 0usize;
    for (idx, ea) in flat_str.split(" | ").enumerate() {
        ea_count += 1;
        let (w_raw, m_raw) = split_eojeol_word_and_analysis(ea);
        let mo_compat = state.normalizer.normalize_morph_for_regex(m_raw);
        let mo_clean = strip_leading_morph_quotes(&mo_compat);

        let mut matched_lbl: Option<&'static str> = None;

        // 1) Regex match (takes priority when query has regex syntax or regex is enabled)
        if qctx.use_regex {
            if req_start {
                if let Some(re_anc) = qctx.anchored_regex {
                    if re_anc.is_match(mo_clean) {
                        matched_lbl = Some(layer_regex_label);
                    }
                }
            } else if let Some(re) = qctx.compiled_regex {
                if re.is_match(mo_clean) {
                    matched_lbl = Some(layer_regex_label);
                }
            }
        }
        // 2) Jamo bracket match (when not a regex-quantifier query)
        if matched_lbl.is_none() && (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
            if let Some(b_tokens) = qctx.bracket_tokens {
                let mo_units = state.normalizer.tokenize_text_units(mo_clean);
                if let Some((sc, _)) = find_bracket_query_match(b_tokens, &mo_units) {
                    if !req_start || sc == 0 {
                        matched_lbl = Some(layer_jamo_label);
                    }
                }
            }
        }
        // 3) Plain text / normalized morph match
        if matched_lbl.is_none() {
            if let Some(ref cq_cln) = cq_clean {
                if req_start {
                    if mo_clean.starts_with(qctx.core_query)
                        || mo_clean.replace(" + ", "+").starts_with(cq_cln.as_str())
                        || state.normalizer.normalize_morph_str(mo_clean).starts_with(qctx.norm_morph_query)
                    {
                        matched_lbl = Some(layer_label);
                    }
                } else if mo_clean.contains(qctx.core_query)
                    || mo_clean.replace(" + ", "+").contains(cq_cln.as_str())
                    || state.normalizer.normalize_morph_str(mo_clean).contains(qctx.norm_morph_query)
                {
                    matched_lbl = Some(layer_label);
                }
            }
        }

        if let Some(lbl) = matched_lbl {
            let w_disp = state.normalizer.compose_modern_hangul_for_display(w_raw);
            let word_spans = compute_display_word_spans(display_text);
            let (sc, ec) =
                find_eojeol_span_in_display(display_text, &word_spans, idx, idx, &w_disp);
            return Some((sc, ec, lbl));
        }
    }

    // If search_unit is strictly "eojeol", do not cross eojeol boundaries!
    if qctx.search_unit == "eojeol" || ea_count < 2 {
        return None;
    }

    // 2. Multi-eojeol matching within the sentence ("sentence" or "text" mode)
    struct EojeolItem {
        raw_word: String,
        mo_clean: String,
    }
    let mut items: Vec<EojeolItem> = Vec::with_capacity(ea_count);
    for ea in flat_str.split(" | ") {
        let (w_raw, m_raw) = split_eojeol_word_and_analysis(ea);
        let w_disp = state.normalizer.compose_modern_hangul_for_display(w_raw);
        let mo_compat = state.normalizer.normalize_morph_for_regex(m_raw);
        let mo_clean = strip_leading_morph_quotes(&mo_compat).to_string();
        items.push(EojeolItem {
            raw_word: w_disp,
            mo_clean,
        });
    }

    // 2. Multi-eojeol matching within the sentence ("sentence" or "text" mode)
    // Build pure morpheme sentence stream `mo_0 | mo_1 | ...` with eojeol index mapping
    for start_idx in 0..items.len() {
        let mut mo_tail = String::new();
        let mut eojeol_end_byte_offsets: Vec<(usize, usize)> = Vec::new();
        for j in start_idx..items.len() {
            if j > start_idx {
                mo_tail.push_str(" | ");
            }
            mo_tail.push_str(&items[j].mo_clean);
            let b_end = mo_tail.len();
            eojeol_end_byte_offsets.push((j, b_end));
        }

        // 2-1) Regex multi-eojeol match
        if qctx.use_regex {
            let re_to_use = if req_start {
                qctx.anchored_regex
            } else {
                qctx.compiled_regex
            };
            if let Some(re) = re_to_use {
                if let Some(m) = re.find(&mo_tail) {
                    if !req_start || m.start() == 0 {
                        let end_ej = eojeol_end_byte_offsets
                            .iter()
                            .find(|&&(_, b_end)| m.end() <= b_end)
                            .map(|&(ej, _)| ej)
                            .unwrap_or(items.len() - 1);
                        let word_spans = compute_display_word_spans(display_text);
                        let (sc, ec) = find_eojeol_span_in_display(
                            display_text,
                            &word_spans,
                            start_idx,
                            end_ej,
                            &items[start_idx].raw_word,
                        );
                        return Some((sc, ec, layer_regex_label));
                    }
                }
            }
        }

        // 2-2) Jamo bracket multi-eojeol match
        if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
            if let Some(b_tokens) = qctx.bracket_tokens {
                let tail_units = state.normalizer.tokenize_text_units(&mo_tail);
                if let Some((sc_u, ec_u)) = find_bracket_query_match(b_tokens, &tail_units) {
                    if !req_start || sc_u == 0 {
                        let matched_byte_len: usize =
                            mo_tail.chars().take(ec_u).map(|c| c.len_utf8()).sum();
                        let end_ej = eojeol_end_byte_offsets
                            .iter()
                            .find(|&&(_, b_end)| matched_byte_len <= b_end)
                            .map(|&(ej, _)| ej)
                            .unwrap_or(items.len() - 1);
                        let word_spans = compute_display_word_spans(display_text);
                        let (sc, ec) = find_eojeol_span_in_display(
                            display_text,
                            &word_spans,
                            start_idx,
                            end_ej,
                            &items[start_idx].raw_word,
                        );
                        return Some((sc, ec, layer_jamo_label));
                    }
                }
            }
        }

        // 2-3) Plain text multi-eojeol match
        if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
            if req_start {
                if mo_tail.starts_with(qctx.core_query) {
                    let word_spans = compute_display_word_spans(display_text);
                    let (sc, ec) = find_eojeol_span_in_display(
                        display_text,
                        &word_spans,
                        start_idx,
                        (start_idx + 1).min(items.len() - 1),
                        &items[start_idx].raw_word,
                    );
                    return Some((sc, ec, layer_label));
                }
            } else if mo_tail.contains(qctx.core_query) {
                let word_spans = compute_display_word_spans(display_text);
                let (sc, ec) = find_eojeol_span_in_display(
                    display_text,
                    &word_spans,
                    start_idx,
                    (start_idx + 1).min(items.len() - 1),
                    &items[start_idx].raw_word,
                );
                return Some((sc, ec, layer_label));
            }
        }

        // In "text" mode (where req_start is false), start_idx == 0 already scanned the entire mo_tail unanchored
        if !req_start {
            break;
        }
    }

    None
}

pub(crate) fn evaluate_segment_match(
    state: &AppState,
    qctx: &SearchQueryContext,
    display_text: &str,
    text_raw: &str,
    text_nfd_norm: &str,
    morph_flat: &str,
    sense_flat: &str,
    syntax_tree: &str,
) -> Option<(usize, usize, &'static str)> {
    let req_start = qctx.req_eojeol_start && qctx.search_unit != "text";

    // 1. Surface layer matching
    if qctx.use_surface {
        if qctx.search_unit == "eojeol" {
            let word_spans = compute_display_word_spans(display_text);
            for &(w_sc, w_ec, w_str) in &word_spans {
                let (w_clean, _skipped_c) = strip_leading_surface_quotes(w_str);
                if w_clean.is_empty() {
                    continue;
                }
                // 1-1) Regex in eojeol mode
                if qctx.use_regex {
                    if req_start {
                        if let Some(re_anc) = qctx.anchored_regex {
                            if let Some(_) = re_anc.find(w_clean) {
                                return Some((w_sc, w_ec, "원문/정규식"));
                            }
                        }
                    } else if let Some(re) = qctx.compiled_regex {
                        if let Some(_) = re.find(w_clean) {
                            return Some((w_sc, w_ec, "원문/정규식"));
                        }
                    }
                }
                // 1-2) Jamo bracket in eojeol mode
                if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
                    if let Some(b_tokens) = qctx.bracket_tokens {
                        let units = state.normalizer.tokenize_text_units(w_clean);
                        if let Some((sc, _ec)) = find_bracket_query_match(b_tokens, &units) {
                            if !req_start || sc == 0 {
                                return Some((w_sc, w_ec, "원문/자소"));
                            }
                        }
                    }
                }
                // 1-3) Text / NFD in eojeol mode
                if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
                    let (norm_chars, char_map) = state.normalizer.to_nfd_norm_with_map(w_clean);
                    let candidates = find_all_char_subslices(&norm_chars, qctx.norm_query_chars);
                    for (n_start, n_end) in candidates {
                        if !qctx.use_jamo && n_end < norm_chars.len() && is_jongseong_jamo(norm_chars[n_end]) {
                            continue;
                        }
                        let orig_start = char_map.get(n_start).copied().unwrap_or(0);
                        let orig_end = char_map
                            .get(n_end.saturating_sub(1))
                            .map(|&idx| idx + 1)
                            .unwrap_or(orig_start + 1);
                        if req_start && orig_start != 0 {
                            continue;
                        }
                        if qctx.has_trailing_space && orig_end < w_clean.chars().count() {
                            continue;
                        }
                        return Some((w_sc, w_ec, "원문/음절"));
                    }
                }
            }
        } else {
            // Sentence or Text unit mode on Surface
            let display_chars: Vec<char> = display_text.chars().collect();
            let is_valid_start = |char_idx: usize| -> bool {
                if !req_start || char_idx == 0 {
                    return true;
                }
                let mut k = char_idx;
                while k > 0 && is_leading_quote_or_bracket(display_chars[k - 1]) {
                    k -= 1;
                }
                k == 0 || display_chars[k - 1].is_whitespace()
            };

            // 1-1) Regex on display_text
            if qctx.use_regex {
                if let Some(re) = qctx.compiled_regex {
                    for m in re.find_iter(display_text) {
                        let start_c = display_text[..m.start()].chars().count();
                        if !is_valid_start(start_c) {
                            continue;
                        }
                        let end_c = start_c + display_text[m.start()..m.end()].chars().count();
                        return Some((start_c, end_c, "원문/정규식"));
                    }
                    if !req_start {
                        if let Some(_m) = re.find(text_nfd_norm) {
                            return Some((0, display_chars.len().min(8), "원문/정규식(NFD)"));
                        }
                    }
                }
            }

            // 1-2) Jamo bracket on display_text
            if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
                if let Some(b_tokens) = qctx.bracket_tokens {
                    let units = state.normalizer.tokenize_text_units(display_text);
                    for start_u in 0..units.len() {
                        if !is_valid_start(units[start_u].start_char) {
                            continue;
                        }
                        if let Some(consumed) = match_tokens_at(b_tokens, &units[start_u..]) {
                            if consumed > 0 {
                                let sc = units[start_u].start_char;
                                let ec = units[start_u + consumed - 1].end_char;
                                return Some((sc, ec, "원문/자소"));
                            }
                        }
                    }
                }
            }

            // 1-3) Text / NFD on display_text
            if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
                let (norm_chars, char_map) = state.normalizer.to_nfd_norm_with_map(display_text);
                let candidates = find_all_char_subslices(&norm_chars, qctx.norm_query_chars);
                for (n_start, n_end) in candidates {
                    if !qctx.use_jamo && n_end < norm_chars.len() && is_jongseong_jamo(norm_chars[n_end]) {
                        continue;
                    }
                    let orig_start = char_map.get(n_start).copied().unwrap_or(0);
                    let orig_end = char_map
                        .get(n_end.saturating_sub(1))
                        .map(|&idx| idx + 1)
                        .unwrap_or(orig_start + 1);
                    if !is_valid_start(orig_start) {
                        continue;
                    }
                    if !check_word_boundary(
                        &display_chars,
                        orig_start,
                        orig_end,
                        qctx.has_leading_space,
                        qctx.has_trailing_space,
                    ) {
                        continue;
                    }
                    return Some((orig_start, orig_end, "원문/음절"));
                }
                if text_raw.contains(qctx.core_query) {
                    if let Some(byte_pos) = display_text.find(qctx.core_query) {
                        let sc = display_text[..byte_pos].chars().count();
                        let ec = sc + qctx.core_query.chars().count();
                        if is_valid_start(sc)
                            && check_word_boundary(
                                &display_chars,
                                sc,
                                ec,
                                qctx.has_leading_space,
                                qctx.has_trailing_space,
                            )
                        {
                            return Some((sc, ec, "원문(PUA)"));
                        }
                    } else if !req_start && !qctx.has_leading_space && !qctx.has_trailing_space {
                        return Some((0, display_chars.len().min(6), "원문(PUA)"));
                    }
                }
            }
        }
    }

    // 2. Morpheme layer matching
    if qctx.use_morph {
        if let Some(res) = match_analysis_layer(
            state,
            qctx,
            display_text,
            morph_flat,
            "형태분석",
            "형태소/자소",
            "형태소/정규식",
        ) {
            return Some(res);
        }
    }

    // 3. Sense layer matching
    if qctx.use_sense {
        if let Some(res) = match_analysis_layer(
            state,
            qctx,
            display_text,
            sense_flat,
            "형태의미",
            "형태의미/자소",
            "형태의미/정규식",
        ) {
            return Some(res);
        }
    }

    // 4. Syntax tree layer matching
    if qctx.use_syntax && !syntax_tree.is_empty() {
        let first_word_len = || {
            display_text
                .split_whitespace()
                .next()
                .map(|w| w.chars().count())
                .unwrap_or(6)
        };
        if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
            if syntax_tree.contains(qctx.core_query) {
                return Some((0, first_word_len(), "구문분석"));
            }
            let syn_compat = state.normalizer.normalize_morph_for_regex(syntax_tree);
            if syn_compat.contains(qctx.core_query)
                || state.normalizer.normalize_morph_str(syntax_tree).contains(qctx.norm_morph_query)
            {
                return Some((0, first_word_len(), "구문분석"));
            }
        } else {
            let syn_compat = state.normalizer.normalize_morph_for_regex(syntax_tree);
            if qctx.use_regex {
                if let Some(re) = qctx.compiled_regex {
                    if re.is_match(&syn_compat) || re.is_match(syntax_tree) {
                        return Some((0, first_word_len(), "구문/정규식"));
                    }
                }
            }
            if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
                if let Some(b_tokens) = qctx.bracket_tokens {
                    let syn_units = state.normalizer.tokenize_text_units(&syn_compat);
                    if find_bracket_query_match(b_tokens, &syn_units).is_some() {
                        return Some((0, first_word_len(), "구문/자소"));
                    }
                }
            }
        }
    }

    None
}

pub(crate) fn evaluate_cross_sentence_match(
    state: &AppState,
    conn: &Connection,
    qctx: &SearchQueryContext,
    doc_id: &str,
    seq_no: i64,
    display_text: &str,
    text_raw: &str,
    text_nfd_norm: &str,
    morph_flat: &str,
    sense_flat: &str,
    syntax_tree: &str,
) -> Option<(String, usize, usize, &'static str)> {
    let mut stmt_next = conn
        .prepare_cached(
            "SELECT text_raw, text_unicode, text_nfd_norm, COALESCE(morph_flat, ''), COALESCE(sense_flat, ''), COALESCE(syntax_tree, '')
             FROM segments WHERE doc_id = ?1 AND seq_no = ?2",
        )
        .ok()?;
    let (next_raw, next_unicode, next_nfd, next_morph, next_sense, next_syntax): (
        String,
        String,
        String,
        String,
        String,
        String,
    ) = stmt_next
        .query_row(params![doc_id, seq_no + 1], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3).unwrap_or_default(),
                r.get(4).unwrap_or_default(),
                r.get(5).unwrap_or_default(),
            ))
        })
        .ok()?;

    let next_display = state
        .normalizer
        .compose_modern_hangul_for_display(&next_unicode);
    let cur_char_len = display_text.chars().count();
    if cur_char_len == 0 || next_display.is_empty() {
        return None;
    }

    let comb_display = format!("{} {}", display_text, next_display);
    let comb_raw = format!("{} {}", text_raw, next_raw);
    let comb_nfd = format!("{} {}", text_nfd_norm, next_nfd);
    let comb_morph = if !morph_flat.is_empty() && !next_morph.is_empty() {
        format!("{} | {}", morph_flat, next_morph)
    } else {
        String::new()
    };
    let comb_sense = if !sense_flat.is_empty() && !next_sense.is_empty() {
        format!("{} | {}", sense_flat, next_sense)
    } else {
        String::new()
    };
    let comb_syntax = if !syntax_tree.is_empty() && !next_syntax.is_empty() {
        format!("{} {}", syntax_tree, next_syntax)
    } else {
        String::new()
    };

    if let Some((sc, ec, lbl)) = evaluate_segment_match(
        state,
        qctx,
        &comb_display,
        &comb_raw,
        &comb_nfd,
        &comb_morph,
        &comb_sense,
        &comb_syntax,
    ) {
        if sc < cur_char_len && ec > cur_char_len {
            return Some((comb_display, sc, ec, lbl));
        }
    }
    None
}

/// 형태소/의미/구문 검색에서 NFD/자소 변형이 없는 순수 ASCII 태그(예: `/NNG`, `NP_SBJ`, `__02`)를 추출해
/// SQLite LIKE 1차 필터로 활용합니다. OR(`|`) 또는 선택 수량자(`?`, `*`, `{0`)가 있는 경우 None을 반환합니다.
fn extract_safe_ascii_filter_token(
    core_query: &str,
    use_morph: bool,
    use_sense: bool,
    use_syntax: bool,
) -> Option<String> {
    if core_query.contains('|') || core_query.contains('(') || core_query.contains('?') || core_query.contains('*') {
        return None;
    }
    if use_syntax && !use_morph && !use_sense {
        let t = core_query.trim();
        if t.len() >= 2 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Some(t.to_string());
        }
        return None;
    }
    if (use_morph || use_sense) && core_query.contains('/') {
        if let Some(after) = core_query.rsplit('/').next() {
            let tag: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            // '/' 바로 뒤가 2글자 이상 영문 대문자 품사 태그(NNG, ETM, VV 등)인 경우 안전
            if tag.len() >= 2 && tag.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                return Some(format!("/{}", tag));
            }
        }
    }
    if use_sense {
        if let Some(pos) = core_query.find("__") {
            let after = &core_query[pos..];
            let tok: String = after
                .chars()
                .take_while(|c| *c == '_' || c.is_ascii_digit())
                .collect();
            if tok.len() >= 4 {
                return Some(tok);
            }
        }
    }
    None
}

pub(crate) fn handle_search(
    state: &AppState,
    params: &HashMap<String, String>,
) -> ApiResponse {
    let t0 = Instant::now();
    let my_search_seq = state.latest_search_seq.fetch_add(1, Ordering::Relaxed) + 1;

    let raw_query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let has_leading_space = raw_query.starts_with(' ') && !raw_query.trim().is_empty();
    let has_trailing_space = raw_query.ends_with(' ') && !raw_query.trim().is_empty();
    let core_query = if has_leading_space || has_trailing_space {
        raw_query.trim()
    } else {
        raw_query
    };

    let search_unit = match params
        .get("search_unit")
        .map(|s| s.trim())
        .unwrap_or("eojeol")
    {
        "sentence" => "sentence",
        "text" => "text",
        _ => "eojeol",
    };

    let targets_param = params.get("targets").map(|s| s.as_str()).unwrap_or("");
    let methods_param = params.get("methods").map(|s| s.as_str()).unwrap_or("");
    let mode = params.get("mode").map(|s| s.as_str()).unwrap_or("surface");

    // Target layers: single layer by default (surface, morph, sense, syntax)
    let mut use_surface = false;
    let mut use_morph = false;
    let mut use_sense = false;
    let mut use_syntax = false;

    if !targets_param.is_empty() {
        for t in targets_param.split(',') {
            match t.trim() {
                "surface" => use_surface = true,
                "morph" => use_morph = true,
                "sense" => use_sense = true,
                "syntax" => use_syntax = true,
                "all" => {
                    use_surface = true;
                    use_morph = true;
                    use_sense = true;
                    use_syntax = true;
                }
                _ => {}
            }
        }
    } else {
        match mode {
            "morph" => use_morph = true,
            "sense" => use_sense = true,
            "syntax" => use_syntax = true,
            _ => use_surface = true,
        }
    }

    // Methods: text, jamo, regex
    let mut use_text = false;
    let mut use_jamo = false;
    let mut use_regex = false;

    if !methods_param.is_empty() {
        for m in methods_param.split(',') {
            match m.trim() {
                "text" => use_text = true,
                "jamo" => use_jamo = true,
                "regex" => use_regex = true,
                _ => {}
            }
        }
    } else {
        match mode {
            "regex" => use_regex = true,
            "jamo" => use_jamo = true,
            _ => {
                use_text = true;
                if core_query.contains('[') && core_query.contains('/') && core_query.contains(']') {
                    use_jamo = true;
                }
            }
        }
    }

    if has_regex_syntax(core_query) {
        use_regex = true;
    }

    if !use_surface && !use_morph && !use_sense && !use_syntax {
        use_surface = true;
    }
    if !use_text && !use_jamo && !use_regex {
        use_text = true;
    }

    let doc_filter = params.get("doc_id").map(|s| s.as_str()).unwrap_or("ALL");
    let doc_ids_raw = params.get("doc_ids").map(|s| s.as_str()).unwrap_or("");
    let selected_doc_ids: Vec<String> = if !doc_ids_raw.is_empty() {
        doc_ids_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "ALL")
            .collect()
    } else if doc_filter != "ALL" && !doc_filter.is_empty() {
        vec![doc_filter.to_string()]
    } else {
        Vec::new()
    };
    let cat_filter = params.get("category").map(|s| s.as_str()).unwrap_or("ALL");
    let layer_filter = params
        .get("layer_filter")
        .map(|s| s.as_str())
        .unwrap_or("ALL");
    let context_words: usize = params
        .get("context_words")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let raw_limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let limit: usize = if raw_limit == 0 {
        usize::MAX
    } else {
        raw_limit
    };

    let norm_query_nfd = state.normalizer.to_nfd_norm(core_query);
    let norm_query_display = state
        .normalizer
        .compose_modern_hangul_for_display(&norm_query_nfd);
    let norm_query_chars: Vec<char> = norm_query_nfd.chars().collect();
    let norm_morph_query = state.normalizer.normalize_morph_str(core_query);

    let bracket_tokens = state.normalizer.parse_bracket_query(core_query);

    if core_query.is_empty()
        && selected_doc_ids.is_empty()
        && layer_filter == "ALL"
        && cat_filter == "ALL"
        && use_surface
    {
        return json_response(
            &SearchResponse {
                query: raw_query.to_string(),
                mode: mode.to_string(),
                normalized_query_nfd: norm_query_nfd,
                normalized_query_display: norm_query_display,
                total_matches: 0,
                returned_count: 0,
                elapsed_ms: 0.0,
                cancelled: false,
                memory_tier: state.memory_profile.tier_label.clone(),
                hits: Vec::new(),
            },
            200,
        );
    }

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };
    state.memory_profile.apply_pragmas(&conn);

    let (compiled_regex, anchored_regex) = if (use_regex || mode == "regex") && !core_query.is_empty() {
        let expanded_regex_str = expand_bracket_syntax_to_regex(core_query);
        let base_pat = match Regex::new(&expanded_regex_str) {
            Ok(_) => expanded_regex_str,
            Err(e) => match Regex::new(core_query) {
                Ok(_) => core_query.to_string(),
                Err(_) => {
                    return json_response(
                        &serde_json::json!({ "error": format!("정규표현식 오류: {}", e) }),
                        400,
                    );
                }
            },
        };
        let r_unanchored = Regex::new(&base_pat).ok();
        let anchored_pat = if base_pat.starts_with('^') {
            base_pat.clone()
        } else {
            format!("^(?:{})", base_pat)
        };
        let r_anchored = Regex::new(&anchored_pat).ok();
        (r_unanchored, r_anchored)
    } else {
        (None, None)
    };

    // 문서 메타데이터 인메모리 캐시 조회
    struct DocMeta {
        title: String,
        category: String,
        layers: String,
    }
    let mut doc_meta_map: HashMap<String, DocMeta> = HashMap::new();
    if let Ok(mut stmt_docs) =
        conn.prepare("SELECT doc_id, title, corpus_category, annotation_layers FROM documents")
    {
        if let Ok(doc_iter) = stmt_docs.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }) {
            for d in doc_iter.flatten() {
                doc_meta_map.insert(
                    d.0,
                    DocMeta {
                        title: d.1,
                        category: d.2,
                        layers: d.3,
                    },
                );
            }
        }
    }

    let mut effective_doc_ids = selected_doc_ids.clone();
    if cat_filter != "ALL" && !cat_filter.is_empty() {
        let cat_matched_ids: Vec<String> = doc_meta_map
            .iter()
            .filter(|(_, meta)| meta.category.contains(cat_filter))
            .map(|(id, _)| id.clone())
            .collect();
        if effective_doc_ids.is_empty() {
            effective_doc_ids = if cat_matched_ids.is_empty() {
                vec!["__NO_MATCHING_DOC__".to_string()]
            } else {
                cat_matched_ids
            };
        } else {
            effective_doc_ids.retain(|id| cat_matched_ids.contains(id));
            if effective_doc_ids.is_empty() {
                effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
            }
        }
    }

    // 검색 대상 층위가 '형태소/품사', '형태의미', '구문표지'인 경우 해당 분석 말뭉치 문서들로만 엄격 제한
    if !use_surface {
        let layer_matched_ids: Vec<String> = doc_meta_map
            .iter()
            .filter(|(_, meta)| {
                (use_morph && (meta.category.contains("형태") || meta.layers.contains("형태")))
                    || (use_sense && (meta.category.contains("의미") || meta.layers.contains("의미")))
                    || (use_syntax && (meta.category.contains("구문") || meta.layers.contains("구문")))
            })
            .map(|(id, _)| id.clone())
            .collect();
        if effective_doc_ids.is_empty() {
            effective_doc_ids = if layer_matched_ids.is_empty() {
                vec!["__NO_MATCHING_DOC__".to_string()]
            } else {
                layer_matched_ids
            };
        } else {
            effective_doc_ids.retain(|id| layer_matched_ids.contains(id));
            if effective_doc_ids.is_empty() {
                effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
            }
        }
    }

    let mut sql = String::from(
        "SELECT s.segment_id, s.doc_id, s.seq_no,
                COALESCE(s.sent_id_str, ''), COALESCE(s.page_info, ''), '', COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                s.text_raw, s.text_unicode, s.text_nfd_norm,
                COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, '')
         FROM segments s
         WHERE 1=1",
    );
    let mut sql_params: Vec<String> = Vec::new();

    if !effective_doc_ids.is_empty() {
        let placeholders = vec!["?"; effective_doc_ids.len()].join(", ");
        sql.push_str(&format!(" AND s.doc_id IN ({})", placeholders));
        for did in &effective_doc_ids {
            sql_params.push(did.clone());
        }
    }

    // 사용자가 선택한 검색 대상 층위에 따른 자동 말뭉치 필터 적용
    if !use_surface {
        let mut target_filters = Vec::new();
        if use_morph {
            target_filters.push("COALESCE(s.morph_flat, '') != ''");
        }
        if use_sense {
            target_filters.push("COALESCE(s.sense_flat, '') != ''");
        }
        if use_syntax {
            target_filters.push("COALESCE(s.syntax_tree, '') != ''");
        }
        if !target_filters.is_empty() {
            sql.push_str(&format!(" AND ({})", target_filters.join(" OR ")));
        }
    }

    // 명시적 레거시 layer_filter 호환
    if layer_filter == "TRIPLE" {
        sql.push_str(" AND COALESCE(s.morph_flat, '') != '' AND COALESCE(s.sense_flat, '') != '' AND COALESCE(s.syntax_tree, '') != ''");
    } else if layer_filter == "OLD_HANGUL" {
        let old_ids: Vec<String> = doc_meta_map
            .iter()
            .filter(|(_, meta)| meta.category.contains("옛한글"))
            .map(|(id, _)| id.clone())
            .collect();
        if !old_ids.is_empty() {
            let ph = vec!["?"; old_ids.len()].join(", ");
            sql.push_str(&format!(" AND s.doc_id IN ({})", ph));
            for oid in old_ids {
                sql_params.push(oid);
            }
        }
    } else if layer_filter == "MORPH" && use_surface {
        sql.push_str(" AND COALESCE(s.morph_flat, '') != ''");
    } else if layer_filter == "SENSE" && use_surface {
        sql.push_str(" AND COALESCE(s.sense_flat, '') != ''");
    } else if layer_filter == "SYNTAX" && use_surface {
        sql.push_str(" AND COALESCE(s.syntax_tree, '') != ''");
    }

    let choseong_only = is_choseong_only_query(&norm_query_chars);
    if !core_query.is_empty()
        && !use_regex
        && mode != "regex"
        && !choseong_only
        && bracket_tokens.is_none()
        && use_surface
        && !use_morph
        && !use_sense
        && !use_syntax
    {
        let like_tok = if search_unit == "text" && core_query.contains(' ') {
            core_query.split_whitespace().next().unwrap_or(core_query)
        } else {
            core_query
        };
        let like_nfd = state.normalizer.to_nfd_norm(like_tok);
        sql.push_str(" AND (s.text_nfd_norm LIKE ? OR s.text_unicode LIKE ? OR s.text_raw LIKE ?)");
        sql_params.push(format!("%{}%", like_nfd));
        sql_params.push(format!("%{}%", like_tok));
        sql_params.push(format!("%{}%", like_tok));
    } else if !core_query.is_empty() && !use_surface {
        if let Some(ascii_tok) = extract_safe_ascii_filter_token(core_query, use_morph, use_sense, use_syntax) {
            let col = if use_morph {
                "s.morph_flat"
            } else if use_sense {
                "s.sense_flat"
            } else {
                "s.syntax_tree"
            };
            sql.push_str(&format!(" AND {} LIKE ?", col));
            sql_params.push(format!("%{}%", ascii_tok));
        }
    }

    sql.push_str(" ORDER BY s.doc_id ASC, s.seq_no ASC");

    if core_query.is_empty() && limit != usize::MAX {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };

    let param_refs: Vec<&dyn rusqlite::ToSql> =
        sql_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let mut hits = Vec::new();
    let mut total_matches = 0usize;
    let mut cancelled = false;
    let mut scanned_rows = 0usize;

    let qctx = SearchQueryContext {
        core_query,
        search_unit,
        req_eojeol_start: query_requires_eojeol_start(core_query),
        has_leading_space,
        has_trailing_space,
        use_surface,
        use_morph,
        use_sense,
        use_syntax,
        use_text,
        use_jamo,
        use_regex,
        norm_query_chars: &norm_query_chars,
        norm_morph_query: &norm_morph_query,
        bracket_tokens: bracket_tokens.as_deref(),
        compiled_regex: compiled_regex.as_ref(),
        anchored_regex: anchored_regex.as_ref(),
    };

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3).unwrap_or_default(),
            row.get::<_, String>(4).unwrap_or_default(),
            row.get::<_, String>(5).unwrap_or_default(),
            row.get::<_, String>(6).unwrap_or_default(),
            row.get::<_, String>(7).unwrap_or_default(),
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11).unwrap_or_default(),
            row.get::<_, String>(12).unwrap_or_default(),
            row.get::<_, String>(13).unwrap_or_default(),
        ))
    });

    if let Ok(iter) = rows {
        for item in iter.flatten() {
            scanned_rows += 1;
            if scanned_rows & 127 == 0 {
                if state.cancel_search_seq.load(Ordering::Relaxed) >= my_search_seq {
                    cancelled = true;
                    break;
                }
            }

            let (
                segment_id,
                doc_id,
                seq_no,
                sent_id,
                page_ref,
                speaker,
                lang,
                sent_type,
                text_raw,
                text_unicode,
                text_nfd_norm,
                morph_flat,
                sense_flat,
                syntax_tree,
            ) = item;

            let display_text = state
                .normalizer
                .compose_modern_hangul_for_display(&text_unicode);

            // Browse mode if query is empty
            if core_query.is_empty() {
                total_matches += 1;
                if hits.len() < limit {
                    let (doc_title, corpus_category, annotation_layers) = match doc_meta_map.get(&doc_id) {
                        Some(m) => (m.title.clone(), m.category.clone(), m.layers.clone()),
                        None => (doc_id.clone(), "원시 말뭉치".to_string(), "원문".to_string()),
                    };
                    let first_word_len = display_text
                        .split_whitespace()
                        .next()
                        .map(|w| w.chars().count())
                        .unwrap_or(1);
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &display_text,
                            0,
                            first_word_len,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    let browse_layer = if !use_surface && use_morph {
                        "형태분석 열람"
                    } else if !use_surface && use_sense {
                        "형태의미 열람"
                    } else if !use_surface && use_syntax {
                        "구문분석 열람"
                    } else {
                        "문서 열람"
                    };
                    hits.push(SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: browse_layer.to_string(),
                    });
                }
                continue;
            }

            let mut matched_span = evaluate_segment_match(
                state,
                &qctx,
                &display_text,
                &text_raw,
                &text_nfd_norm,
                &morph_flat,
                &sense_flat,
                &syntax_tree,
            )
            .map(|(sc, ec, lbl)| (display_text.clone(), sc, ec, lbl));

            if matched_span.is_none() && search_unit == "text" {
                matched_span = evaluate_cross_sentence_match(
                    state,
                    &conn,
                    &qctx,
                    &doc_id,
                    seq_no,
                    &display_text,
                    &text_raw,
                    &text_nfd_norm,
                    &morph_flat,
                    &sense_flat,
                    &syntax_tree,
                );
            }

            if let Some((kwic_base_text, start_c, end_c, layer_name)) = matched_span {
                total_matches += 1;
                if hits.len() < limit {
                    let (doc_title, corpus_category, annotation_layers) = match doc_meta_map.get(&doc_id) {
                        Some(m) => (m.title.clone(), m.category.clone(), m.layers.clone()),
                        None => (doc_id.clone(), "원시 말뭉치".to_string(), "원문".to_string()),
                    };
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &kwic_base_text,
                            start_c,
                            end_c,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    hits.push(SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: layer_name.to_string(),
                    });
                }
            }
        }
    }

    let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
    let returned = hits.len();

    json_response(
        &SearchResponse {
            query: raw_query.to_string(),
            mode: mode.to_string(),
            normalized_query_nfd: norm_query_nfd,
            normalized_query_display: norm_query_display,
            total_matches,
            returned_count: returned,
            elapsed_ms: elapsed,
            cancelled,
            memory_tier: state.memory_profile.tier_label.clone(),
            hits,
        },
        200,
    )
}


pub(crate) fn handle_search_stream(
    state: &Arc<AppState>,
    params: &HashMap<String, String>,
) -> ApiResponse {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(128);
    let state_worker = Arc::clone(state);
    let params_worker = params.clone();

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let my_search_seq = state_worker.latest_search_seq.fetch_add(1, Ordering::Relaxed) + 1;

        let raw_query = params_worker.get("q").map(|s| s.as_str()).unwrap_or("");
        let has_leading_space = raw_query.starts_with(' ') && !raw_query.trim().is_empty();
        let has_trailing_space = raw_query.ends_with(' ') && !raw_query.trim().is_empty();
        let core_query = if has_leading_space || has_trailing_space {
            raw_query.trim()
        } else {
            raw_query
        };

        let search_unit = match params_worker
            .get("search_unit")
            .map(|s| s.trim())
            .unwrap_or("eojeol")
        {
            "sentence" => "sentence",
            "text" => "text",
            _ => "eojeol",
        };

        let targets_param = params_worker.get("targets").map(|s| s.as_str()).unwrap_or("");
        let methods_param = params_worker.get("methods").map(|s| s.as_str()).unwrap_or("");
        let mode = params_worker.get("mode").map(|s| s.as_str()).unwrap_or("surface");

        let mut use_surface = false;
        let mut use_morph = false;
        let mut use_sense = false;
        let mut use_syntax = false;

        if !targets_param.is_empty() {
            for t in targets_param.split(',') {
                match t.trim() {
                    "surface" => use_surface = true,
                    "morph" => use_morph = true,
                    "sense" => use_sense = true,
                    "syntax" => use_syntax = true,
                    "all" => {
                        use_surface = true;
                        use_morph = true;
                        use_sense = true;
                        use_syntax = true;
                    }
                    _ => {}
                }
            }
        } else {
            match mode {
                "morph" => use_morph = true,
                "sense" => use_sense = true,
                "syntax" => use_syntax = true,
                _ => use_surface = true,
            }
        }

        let mut use_text = false;
        let mut use_jamo = false;
        let mut use_regex = false;

        if !methods_param.is_empty() {
            for m in methods_param.split(',') {
                match m.trim() {
                    "text" => use_text = true,
                    "jamo" => use_jamo = true,
                    "regex" => use_regex = true,
                    _ => {}
                }
            }
        } else {
            match mode {
                "regex" => use_regex = true,
                "jamo" => use_jamo = true,
                _ => {
                    use_text = true;
                    if core_query.contains('[') && core_query.contains('/') && core_query.contains(']') {
                        use_jamo = true;
                    }
                }
            }
        }

        if has_regex_syntax(core_query) {
            use_regex = true;
        }

        if !use_surface && !use_morph && !use_sense && !use_syntax {
            use_surface = true;
        }
        if !use_text && !use_jamo && !use_regex {
            use_text = true;
        }

        let doc_filter = params_worker.get("doc_id").map(|s| s.as_str()).unwrap_or("ALL");
        let doc_ids_raw = params_worker.get("doc_ids").map(|s| s.as_str()).unwrap_or("");
        let selected_doc_ids: Vec<String> = if !doc_ids_raw.is_empty() {
            doc_ids_raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "ALL")
                .collect()
        } else if doc_filter != "ALL" && !doc_filter.is_empty() {
            vec![doc_filter.to_string()]
        } else {
            Vec::new()
        };
        let cat_filter = params_worker.get("category").map(|s| s.as_str()).unwrap_or("ALL");
        let layer_filter = params_worker.get("layer_filter").map(|s| s.as_str()).unwrap_or("ALL");
        let context_words: usize = params_worker.get("context_words").and_then(|s| s.parse().ok()).unwrap_or(5);

        let raw_limit: usize = params_worker.get("limit").and_then(|s| s.parse().ok()).unwrap_or(500);
        let limit: usize = if raw_limit == 0 { usize::MAX } else { raw_limit };

        let norm_query_nfd = state_worker.normalizer.to_nfd_norm(core_query);
        let norm_query_display = state_worker.normalizer.compose_modern_hangul_for_display(&norm_query_nfd);
        let norm_query_chars: Vec<char> = norm_query_nfd.chars().collect();
        let norm_morph_query = state_worker.normalizer.normalize_morph_str(core_query);
        let bracket_tokens = state_worker.normalizer.parse_bracket_query(core_query);

        // Send start message
        let start_msg = serde_json::json!({
            "type": "start",
            "query": raw_query,
            "mode": mode,
            "search_unit": search_unit,
            "normalized_query_display": norm_query_display,
            "limit": limit,
        });
        if tx.send(format!("{}\n", start_msg).into_bytes()).is_err() {
            return;
        }

        let conn = match Connection::open(&state_worker.db_path) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = serde_json::json!({ "type": "error", "error": e.to_string() });
                let _ = tx.send(format!("{}\n", err_msg).into_bytes());
                return;
            }
        };
        state_worker.memory_profile.apply_pragmas(&conn);

        let (compiled_regex, anchored_regex) = if (use_regex || mode == "regex") && !core_query.is_empty() {
            let expanded_regex_str = expand_bracket_syntax_to_regex(core_query);
            let base_pat = match Regex::new(&expanded_regex_str) {
                Ok(_) => expanded_regex_str,
                Err(e) => match Regex::new(core_query) {
                    Ok(_) => core_query.to_string(),
                    Err(_) => {
                        let err_msg = serde_json::json!({ "type": "error", "error": format!("정규표현식 오류: {}", e) });
                        let _ = tx.send(format!("{}\n", err_msg).into_bytes());
                        return;
                    }
                },
            };
            let r_unanchored = Regex::new(&base_pat).ok();
            let anchored_pat = if base_pat.starts_with('^') {
                base_pat.clone()
            } else {
                format!("^(?:{})", base_pat)
            };
            let r_anchored = Regex::new(&anchored_pat).ok();
            (r_unanchored, r_anchored)
        } else {
            (None, None)
        };

        struct DocMeta {
            title: String,
            category: String,
            layers: String,
        }
        let mut doc_meta_map: HashMap<String, DocMeta> = HashMap::new();
        if let Ok(mut stmt_docs) = conn.prepare("SELECT doc_id, title, corpus_category, annotation_layers FROM documents") {
            if let Ok(doc_iter) = stmt_docs.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            }) {
                for d in doc_iter.flatten() {
                    doc_meta_map.insert(d.0, DocMeta { title: d.1, category: d.2, layers: d.3 });
                }
            }
        }

        let mut effective_doc_ids = selected_doc_ids.clone();
        if cat_filter != "ALL" && !cat_filter.is_empty() {
            let cat_matched_ids: Vec<String> = doc_meta_map.iter()
                .filter(|(_, meta)| meta.category.contains(cat_filter))
                .map(|(id, _)| id.clone())
                .collect();
            if effective_doc_ids.is_empty() {
                effective_doc_ids = if cat_matched_ids.is_empty() {
                    vec!["__NO_MATCHING_DOC__".to_string()]
                } else {
                    cat_matched_ids
                };
            } else {
                effective_doc_ids.retain(|id| cat_matched_ids.contains(id));
                if effective_doc_ids.is_empty() {
                    effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
                }
            }
        }

        // 검색 대상 층위가 '형태소/품사', '형태의미', '구문표지'인 경우 해당 분석 말뭉치 문서들로만 엄격 제한
        if !use_surface {
            let layer_matched_ids: Vec<String> = doc_meta_map
                .iter()
                .filter(|(_, meta)| {
                    (use_morph && (meta.category.contains("형태") || meta.layers.contains("형태")))
                        || (use_sense && (meta.category.contains("의미") || meta.layers.contains("의미")))
                        || (use_syntax && (meta.category.contains("구문") || meta.layers.contains("구문")))
                })
                .map(|(id, _)| id.clone())
                .collect();
            if effective_doc_ids.is_empty() {
                effective_doc_ids = if layer_matched_ids.is_empty() {
                    vec!["__NO_MATCHING_DOC__".to_string()]
                } else {
                    layer_matched_ids
                };
            } else {
                effective_doc_ids.retain(|id| layer_matched_ids.contains(id));
                if effective_doc_ids.is_empty() {
                    effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
                }
            }
        }

        let mut sql = String::from(
            "SELECT s.segment_id, s.doc_id, s.seq_no,
                    COALESCE(s.sent_id_str, ''), COALESCE(s.page_info, ''), '', COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                    s.text_raw, s.text_unicode, s.text_nfd_norm,
                    COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, '')
             FROM segments s
             WHERE 1=1",
        );
        let mut sql_params: Vec<String> = Vec::new();

        if !effective_doc_ids.is_empty() {
            let placeholders = vec!["?"; effective_doc_ids.len()].join(", ");
            sql.push_str(&format!(" AND s.doc_id IN ({})", placeholders));
            for did in &effective_doc_ids {
                sql_params.push(did.clone());
            }
        }

        if !use_surface {
            let mut target_filters = Vec::new();
            if use_morph {
                target_filters.push("COALESCE(s.morph_flat, '') != ''");
            }
            if use_sense {
                target_filters.push("COALESCE(s.sense_flat, '') != ''");
            }
            if use_syntax {
                target_filters.push("COALESCE(s.syntax_tree, '') != ''");
            }
            if !target_filters.is_empty() {
                sql.push_str(&format!(" AND ({})", target_filters.join(" OR ")));
            }
        }

        if layer_filter == "TRIPLE" {
            sql.push_str(" AND COALESCE(s.morph_flat, '') != '' AND COALESCE(s.sense_flat, '') != '' AND COALESCE(s.syntax_tree, '') != ''");
        } else if layer_filter == "OLD_HANGUL" {
            let old_ids: Vec<String> = doc_meta_map.iter()
                .filter(|(_, meta)| meta.category.contains("옛한글"))
                .map(|(id, _)| id.clone())
                .collect();
            if !old_ids.is_empty() {
                let ph = vec!["?"; old_ids.len()].join(", ");
                sql.push_str(&format!(" AND s.doc_id IN ({})", ph));
                for oid in old_ids {
                    sql_params.push(oid);
                }
            }
        } else if layer_filter == "MORPH" && use_surface {
            sql.push_str(" AND COALESCE(s.morph_flat, '') != ''");
        } else if layer_filter == "SENSE" && use_surface {
            sql.push_str(" AND COALESCE(s.sense_flat, '') != ''");
        } else if layer_filter == "SYNTAX" && use_surface {
            sql.push_str(" AND COALESCE(s.syntax_tree, '') != ''");
        }

        let choseong_only = is_choseong_only_query(&norm_query_chars);
        if !core_query.is_empty()
            && !use_regex
            && mode != "regex"
            && !choseong_only
            && bracket_tokens.is_none()
            && use_surface
            && !use_morph
            && !use_sense
            && !use_syntax
        {
            let like_tok = if search_unit == "text" && core_query.contains(' ') {
                core_query.split_whitespace().next().unwrap_or(core_query)
            } else {
                core_query
            };
            let like_nfd = state_worker.normalizer.to_nfd_norm(like_tok);
            sql.push_str(" AND (s.text_nfd_norm LIKE ? OR s.text_unicode LIKE ? OR s.text_raw LIKE ?)");
            sql_params.push(format!("%{}%", like_nfd));
            sql_params.push(format!("%{}%", like_tok));
            sql_params.push(format!("%{}%", like_tok));
        } else if !core_query.is_empty() && !use_surface {
            if let Some(ascii_tok) = extract_safe_ascii_filter_token(core_query, use_morph, use_sense, use_syntax) {
                let col = if use_morph {
                    "s.morph_flat"
                } else if use_sense {
                    "s.sense_flat"
                } else {
                    "s.syntax_tree"
                };
                sql.push_str(&format!(" AND {} LIKE ?", col));
                sql_params.push(format!("%{}%", ascii_tok));
            }
        }

        sql.push_str(" ORDER BY s.doc_id ASC, s.seq_no ASC");

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                let err_msg = serde_json::json!({ "type": "error", "error": e.to_string() });
                let _ = tx.send(format!("{}\n", err_msg).into_bytes());
                return;
            }
        };

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            sql_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let mut returned_count = 0usize;
        let mut total_matches = 0usize;
        let mut cancelled = false;
        let mut scanned_rows = 0usize;

        let qctx = SearchQueryContext {
            core_query,
            search_unit,
            req_eojeol_start: query_requires_eojeol_start(core_query),
            has_leading_space,
            has_trailing_space,
            use_surface,
            use_morph,
            use_sense,
            use_syntax,
            use_text,
            use_jamo,
            use_regex,
            norm_query_chars: &norm_query_chars,
            norm_morph_query: &norm_morph_query,
            bracket_tokens: bracket_tokens.as_deref(),
            compiled_regex: compiled_regex.as_ref(),
            anchored_regex: anchored_regex.as_ref(),
        };

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3).unwrap_or_default(),
                row.get::<_, String>(4).unwrap_or_default(),
                row.get::<_, String>(5).unwrap_or_default(),
                row.get::<_, String>(6).unwrap_or_default(),
                row.get::<_, String>(7).unwrap_or_default(),
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11).unwrap_or_default(),
                row.get::<_, String>(12).unwrap_or_default(),
                row.get::<_, String>(13).unwrap_or_default(),
            ))
        });

        if let Ok(iter) = rows {
            for item in iter.flatten() {
                scanned_rows += 1;
                if scanned_rows & 63 == 0 {
                    if state_worker.cancel_search_seq.load(Ordering::Relaxed) >= my_search_seq {
                        cancelled = true;
                        break;
                    }
                }

                let (
                    segment_id,
                    doc_id,
                    seq_no,
                    sent_id,
                    page_ref,
                    speaker,
                    lang,
                    sent_type,
                    text_raw,
                    text_unicode,
                    text_nfd_norm,
                    morph_flat,
                    sense_flat,
                    syntax_tree,
                ) = item;

                let display_text = state_worker
                    .normalizer
                    .compose_modern_hangul_for_display(&text_unicode);

                // Browse mode if query is empty
                if core_query.is_empty() {
                    total_matches += 1;
                    returned_count += 1;
                    let (doc_title, corpus_category, annotation_layers) = match doc_meta_map.get(&doc_id) {
                        Some(m) => (m.title.clone(), m.category.clone(), m.layers.clone()),
                        None => (doc_id.clone(), "원시 말뭉치".to_string(), "원문".to_string()),
                    };

                    let first_word_len = display_text
                        .split_whitespace()
                        .next()
                        .map(|w| w.chars().count())
                        .unwrap_or(1);
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &display_text,
                            0,
                            first_word_len,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    let browse_layer = if !use_surface && use_morph {
                        "형태분석 열람"
                    } else if !use_surface && use_sense {
                        "형태의미 열람"
                    } else if !use_surface && use_syntax {
                        "구문분석 열람"
                    } else {
                        "문서 열람"
                    };
                    let hit = SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: browse_layer.to_string(),
                    };

                    let hit_msg = serde_json::json!({
                        "type": "hit",
                        "hit": hit,
                        "matched_count": total_matches,
                    });
                    if tx.send(format!("{}\n", hit_msg).into_bytes()).is_err() {
                        return; // client closed stream
                    }

                    if returned_count >= limit {
                        break;
                    }
                    continue;
                }

                let mut matched_span = evaluate_segment_match(
                    &state_worker,
                    &qctx,
                    &display_text,
                    &text_raw,
                    &text_nfd_norm,
                    &morph_flat,
                    &sense_flat,
                    &syntax_tree,
                )
                .map(|(sc, ec, lbl)| (display_text.clone(), sc, ec, lbl));

                if matched_span.is_none() && search_unit == "text" {
                    matched_span = evaluate_cross_sentence_match(
                        &state_worker,
                        &conn,
                        &qctx,
                        &doc_id,
                        seq_no,
                        &display_text,
                        &text_raw,
                        &text_nfd_norm,
                        &morph_flat,
                        &sense_flat,
                        &syntax_tree,
                    );
                }

                // Hit found!
                if let Some((kwic_base_text, match_start, match_end, layer_name)) = matched_span {
                    total_matches += 1;
                    returned_count += 1;
                    let (doc_title, corpus_category, annotation_layers) = match doc_meta_map.get(&doc_id) {
                        Some(m) => (m.title.clone(), m.category.clone(), m.layers.clone()),
                        None => (doc_id.clone(), "원시 말뭉치".to_string(), "원문".to_string()),
                    };

                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &kwic_base_text,
                            match_start,
                            match_end,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    let hit = SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: layer_name.to_string(),
                    };

                    // [핵심] 검색된 즉시 실시간 스트리밍 전송!
                    let hit_msg = serde_json::json!({
                        "type": "hit",
                        "hit": hit,
                        "matched_count": total_matches,
                    });
                    if tx.send(format!("{}\n", hit_msg).into_bytes()).is_err() {
                        return; // client closed connection
                    }

                    if returned_count >= limit {
                        break;
                    }
                }
            }
        }

        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        let done_msg = serde_json::json!({
            "type": "done",
            "total_matches": total_matches,
            "returned_count": returned_count,
            "elapsed_ms": elapsed,
            "cancelled": cancelled,
        });
        let _ = tx.send(format!("{}\n", done_msg).into_bytes());
    });

    let mut out = Vec::new();
    while let Ok(chunk) = rx.recv() {
        if chunk.is_empty() {
            break;
        }
        out.extend_from_slice(&chunk);
    }
    binary_response(200, "application/x-ndjson; charset=utf-8", out)
}

