# 말뭉치 원본 폴더 구조 계층 트리 및 다중 문서 체크박스 검색 기능 가이드

본 문서는 사용자의 요구사항(검색창 상단 폴딩 버튼 노출 및 직관화, '문서지정'을 원래 코퍼스 저장 폴더 구조의 계층 트리와 체크박스 다중 선택 방식으로 개편)에 따른 아키텍처 및 UI/UX 구현 내역, 백엔드 다중 문서 검색 API 지원, 사용 방법을 상세히 정리한 문서입니다.

---

## 1. 요구사항 및 개선 목표

1. **검색창 상단 옵션 폴딩 직관화 및 버튼 보강**:
   - 기존에는 상단 카드 헤더에만 폴딩 링크가 작게 표시되어 사용자가 찾기 어려웠던 점을 개선.
   - 사용자의 주 시선 영역인 **검색창 입력줄(Row 2) 자체에 `[🔼 검색옵션 접기]` / `[🔽 검색옵션 펼치기]` 인라인 버튼을 신설**하여 언제든 원클릭으로 상단 옵션 바를 여닫을 수 있도록 구현.
   - 상단 카드 헤더와 검색창 줄 버튼 간 실시간 양방향 동기화 및 `localStorage` 브라우저 상태 보존.

2. **'문서지정' 메뉴를 원본 코퍼스 폴더 구조 계층 트리 뷰 + 체크박스 다중 선택으로 전면 개편**:
   - 기존의 단순한 단일 선택 드롭다운(`<select>`)을 폐지.
   - 원본 코퍼스가 저장되어 있던 **실제 디렉터리 경로(폴더명 및 하위 파일 목록)**를 계층적 트리 형태로 시각화.
   - 폴더 및 파일 단위 체크박스(`[✔]`)를 제공하여:
     - 폴더 앞 체크박스 클릭 시 하위 모든 파일 일괄 선택/해제 (일부 선택 시 `indeterminate` 3상태 표시 지원).
     - 개별 파일 체크박스를 통해 필요한 파일들만 자유롭게 복수 선택.
     - `[☑️ 전체 선택]`, `[⬜ 전체 해제]`, `[📂 모두 펼치기]`, `[📁 모두 접기]`.
     - 실시간 파일명/문서명 검색 필터 입력창 제공.
   - **선택된 여러 파일(복수 문서)에서 사용자가 찾는 형식을 한 번에 검색**할 수 있도록 백엔드와 프론트엔드를 완전 연동.

---

## 2. 세부 구현 내역

### 2.1 검색창 상단 옵션 폴딩 기능 강화 ([`web/index.html`](file:///c:/AG/concordance/web/index.html), [`web/styles.css`](file:///c:/AG/concordance/web/styles.css), [`web/app.js`](file:///c:/AG/concordance/web/app.js))

- **인라인 토글 버튼 신설**:
  - `web/index.html`의 검색 입력줄(`search-input-row`) 우측에 `#btn-toggle-options-inline` 버튼 추가.
  - 스타일: 시각적으로 눈에 확 들어오는 연한 블루 테마(`background: #eff6ff; border: 1px solid #93c5fd; color: #1d4ed8; font-weight: 600;`).
- **양방향 동기화 핸들러**:
  - 상단 헤더 버튼(`#btn-toggle-search-options`)과 인라인 버튼(`#btn-toggle-options-inline`) 중 어느 것을 클릭해도 `searchOptionsBar.classList.toggle('collapsed')`가 실행되고, 양쪽의 아이콘(`🔼` ↔ `🔽`)과 텍스트(`검색옵션 접기` ↔ `검색옵션 펼치기`)가 동시에 동기화됨.
  - `localStorage`의 `sconc_search_options_collapsed` 키에 저장되어 프로그램을 재시작하거나 새로고침해도 사용자가 설정한 접힘/펼침 상태가 그대로 유지됨.

---

### 2.2 원본 폴더 구조 계층 트리 뷰 모달 (`#modal-doc-selector`)

- **트리거 버튼 (`#btn-open-doc-selector`)**:
  - 검색 옵션 바의 '문서지정' 항목에 배치:
    - 전체 선택 시: `📂 전체 문서 (13개 파일) ▾`
    - 일부 선택 시: `📂 3개 파일 선택됨 ▾` (블루 강조색)
- **폴더 트리 데이터 추출 및 렌더링 알고리즘 (`buildFolderTreeData`)**:
  - `source_files` 테이블의 `rel_path`(상대 경로)를 `/` 또는 `\` 기준으로 분석하여 폴더명(상위 디렉터리 경로)과 파일명을 추출.
  - 예:
    - `sample/BTAA0001.txt` -> 📁 `sample` 폴더 아래 `📄 BTAA0001.txt`
    - `sample/간이벽온방.xml` -> 📁 `sample` 폴더 아래 `📄 간이벽온방.xml`
    - 사용자가 지정한 폴더로 생성한 DB의 경우 사용자 지정 폴더의 하위 경로가 그대로 트리로 복원됨.
- **모달 기능 구성**:
  1. **폴더 헤더**:
     - 클릭 시 하위 파일 목록 접기/펼치기 (`doc-file-list.collapsed` 토글).
     - 폴더 체크박스: 하위 파일 전체 선택/해제. 하위 파일 중 일부만 선택된 경우 브라우저 표준 `indeterminate` 상태(네모 점 표시) 자동 반영.
  2. **개별 파일 항목**:
     - 파일명 태그(`doc-file-name`), 문서 제목, 레이어 뱃지(`옛한글`, `형태`, `3중연동`), 문장 수 및 어절 수 표시.
     - 선택 시 즉시 하단 카운터와 연동.
  3. **실시간 검색 필터 (`#doc-search-filter`)**:
     - 파일명이나 문서 제목을 입력하면 실시간으로 일치하는 파일만 남기고 필터링.
  4. **하단 선택 요약**:
     - `선택된 파일: 3개 / 전체 13개 (약 4,500문장)` 실시간 집계.
     - `[✓ 선택한 파일로 검색 적용]` 버튼을 누르면 확정 반영.

---

### 2.3 백엔드 다중 문서 검색 API 구현 ([`engine/src/main.rs`](file:///c:/AG/concordance/engine/src/main.rs))

- **파라미터 확장**:
  - `/api/search`에 `doc_ids` 매개변수 지원 추가 (콤마 구분자 형태, 예: `doc_ids=OLD_GANI_1525,OLD_HUNMONG_1527`).
- **SQL 쿼리 동적 생성**:
  ```rust
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

  if !selected_doc_ids.is_empty() {
      let placeholders = vec!["?"; selected_doc_ids.len()].join(", ");
      sql.push_str(&format!(" AND s.doc_id IN ({})", placeholders));
      for did in &selected_doc_ids {
          sql_params.push(did.clone());
      }
  }
  ```
- **다중 탭 연동**:
  - 각 검색 탭 객체(`searchTabs`)에 `docIds: new Set(selectedDocIds)`를 저장하여, 탭을 전환하더라도 당시 검색했던 문서 선택 상태가 완벽히 복원됨.

---

## 3. 검증 결과

1. **Rust 단위 테스트 (`cargo test`)**:
   - `test_syllable_vs_jamo_matching`: 통과 (ok)
   - `test_space_word_boundary_rules`: 통과 (ok)
2. **릴리스 바이너리 빌드 및 자가 검증**:
   - `cargo build --release` 컴파일 완료.
   - `sconc.exe --self-test`: DB 13개 문서, 16개 파일, 40,343개 문장 및 정규식/브래킷 자소 검색 100% 정상 검증 통과.
3. **프론트엔드 구문 검증**:
   - `node -c web/app.js`: 문법 에러 0건 정상 통과.
