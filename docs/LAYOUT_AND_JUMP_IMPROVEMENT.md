# SCONC 화면 레이아웃 최적화 및 원문 정밀 점프·PC 에디터 연동 개선 보고서

본 문서는 사용자의 요구사항(작은 화면에서의 검색 결과 축소 방지 및 상단 폴딩, 원문 점프 시 정확한 문장 위치 추적 및 각 PC의 기본 텍스트 에디터 실행)에 따른 문제 원인 분석, 아키텍처 및 알고리즘 개선, 파일별 수정 내역, 검증 결과를 상세히 정리한 문서입니다.

---

## 1. 요구사항 개요

1. **소형 모니터 레이아웃 최적화 & 검색창 상단 폴딩**:
   - 화면이 작은 컴퓨터(노트북 등)에서 구동 시 상단 헤더와 검색창 옵션에 밀려 말뭉치 검색 결과 테이블이 거의 보이지 않을 정도로 작게 압축되는 현상 해결.
   - 기본 크기(`min-height`)를 정해 놓고 그 이하로는 축소되지 않도록 방지하며, 모니터가 작다면 세로 스크롤을 통해 전체 내용을 확인할 수 있도록 개선.
   - 검색창 상단의 다양한 검색 옵션(검색방식, 표기 층위, 문서 필터, 문맥 어절 수 등) 영역을 사용자가 원할 때 접거나 펼칠 수 있는 **폴딩(Folding)** 기능 구현 및 상태 저장(`localStorage`).
2. **원문 점프 시 정확한 위치 이동 & PC 기본 텍스트 에디터 자동 실행**:
   - 검색 결과에서 '원문 점프' 실행 시, 기존에 1번 라인(헤더)으로만 이동하던 버그를 해결하고 **원본 텍스트 파일의 정확한 해당 문장 위치(Line 번호)**로 이동하도록 정밀 추적 알고리즘 구축.
   - 각 사용자의 PC에 설치된 **기본 텍스트 에디터(VS Code, Notepad++, Windows 메모장 등)**를 통해 해당 원본 파일과 라인이 직접 열리도록 Rust 백엔드 프로세스 연동.
   - 웹 뷰어 모달에서도 열린 에디터 상태를 표시하고, 필요 시 다시 에디터로 열 수 있는 버튼 제공.

---

## 2. 화면 레이아웃 최적화 및 검색창 폴딩 (요구사항 1)

### 2.1 기존 문제점 분석
- 기존 CSS에 `html, body { overflow: hidden; height: 100vh; }` 및 `.app-shell { height: 100vh; overflow: hidden; }`이 적용되어 있어 브라우저 뷰포트 높이가 낮아지면 내부 flex 아이템들이 일괄 축소되었습니다.
- 검색 결과 테이블 래퍼(`.kwic-table-wrap`)에 명시적인 `min-height`가 없어, 세로 해상도가 768px 이하인 노트북 모니터나 브라우저 창을 줄였을 때 검색 결과 행이 1~2개만 보이거나 가려지는 심각한 사용성 문제가 발생했습니다.

### 2.2 개선 구현 내용

#### (1) 브라우저 세로 스크롤 및 최소 높이 보장 (`web/styles.css`)
- **스크롤 허용**: `html, body` 및 `.app-shell`의 `overflow: hidden; height: 100vh;` 제약을 해제하고 `min-height: 100vh; height: auto; overflow-y: auto;`로 전환하여 브라우저 전체 세로 스크롤이 자연스럽게 동작하도록 개선.
- **최소 높이(`min-height`) 단계별 적용**:
  - `.main-container`: `min-height: 650px;`
  - `.tab-panel`: `min-height: 600px;`
  - `.results-area`: `min-height: 480px;`
  - `.kwic-panel`: `min-height: 460px;`
  - `.kwic-table-wrap`: `min-height: 360px; overflow-y: auto;`
  이를 통해 모니터 해상도나 창 크기가 아무리 작아져도 검색 결과 테이블 영역은 최소 360px 이상의 크기를 영구적으로 유지하며 축소되지 않습니다.

#### (2) 검색창 상단 옵션 접기/펼치기 (폴딩) 기능 (`web/index.html`, `web/styles.css`, `web/app.js`)
- **헤더 컨트롤 추가 (`web/index.html`)**:
  - 검색 카드 상단에 접기/펼치기 제어 버튼 `#btn-toggle-search-options` 배치:
    ```html
    <div class="search-card-header">
      <span class="search-card-title">🔍 말뭉치 검색 조건 및 필터 설정</span>
      <button type="button" class="btn-fold-link" id="btn-toggle-search-options" title="검색 옵션을 접거나 펼쳐 화면 공간을 확보합니다.">
        <span id="fold-icon">🔼</span> <span id="fold-text">검색창 상단 옵션 접기</span>
      </button>
    </div>
    ```
- **CSS 애니메이션 및 접힘 스타일 (`web/styles.css`)**:
  - `.search-options-bar.collapsed { display: none !important; }`를 정의하고, 접기 버튼에 hover 및 active 인터랙션 스타일 적용.
- **상태 보존 및 토글 로직 (`web/app.js`)**:
  - 버튼 클릭 시 `#search-options-bar`의 `collapsed` 클래스를 토글하고, 아이콘과 텍스트(`🔼 검색창 상단 옵션 접기` ↔ `🔽 검색창 상단 옵션 펼치기`)를 동적 갱신.
  - 브라우저의 `localStorage.setItem('sconc_search_options_collapsed', '1')`에 상태를 저장하여 페이지를 새로고침하거나 프로그램을 다시 실행해도 사용자가 설정한 접힘/펼침 상태가 그대로 유지되도록 구현.

---

## 3. 원문 정밀 점프 및 PC 에디터 자동 실행 (요구사항 2)

### 3.1 기존 문제점 분석
- **1번 라인으로만 이동하던 원인**:
  - `훈몽자회.xml`: 원본 XML 파일에는 `<c><g>天</g><m>하</m><s>텬</s></c>` 형태로 표기되어 있으나, DB의 `text_raw`에는 `'天 : [훈] 하 / [음] 텬'` 형식으로 파싱되어 저장됨.
  - `간이벽온방.xml`: 원본 파일에는 `<sent type="main" page="1a" n="1">...</sent>`처럼 태그가 감싸져 있으나 DB에는 태그가 벗겨진 순수 텍스트만 저장됨.
  - 기존 백엔드 코드는 파일의 각 줄에 대해 단순 `line.contains(&text_raw)`만 검사했기 때문에 XML 태그가 있거나 구조화된 파일에서는 100% 매칭에 실패하였고, fallback으로 파일의 맨 앞 30줄(1번 라인부터)을 강제 반환하여 헤더만 보였습니다.
- **PC 기본 에디터 미연동**:
  - 기존에는 웹 UI 상의 모달 팝업으로만 원문 전후 줄을 렌더링하고 있었으며, 사용자의 PC에 설치된 텍스트 에디터(VS Code, Notepad++ 등)를 직접 구동하는 기능이 없었습니다.

### 3.2 개선 구현 내용

#### (1) 4단계 원문 정밀 위치 추적 알고리즘 (`engine/src/main.rs: find_target_line_in_source`)
다양한 말뭉치 형식(세종 TEI, XML 태그형, 사전류 등)에서 100% 정확하게 해당 문장의 행 번호를 찾아내는 4단계 탐색 알고리즘을 구축했습니다:

1. **1단계: `sent_id` 태그 일치 검사**
   - 세종 말뭉치 및 표준 XML 말뭉치(`sent_id` 보유 자료)에서 `sent="sId"` 또는 `id="sId"`, `n="sId"`를 포함하는 행을 우선 탐색.
2. **2단계: 구두점/공백 정규화 텍스트 포함 검사**
   - 구두점(`.,?!;:()[]<>"'`) 및 공백을 제거한 `clean_raw`를 생성하여, XML 태그가 없는 원시 말뭉치 및 일반 텍스트에서 포함 여부 확인.
3. **3단계: 사전류 및 한자 표제어 태그 일치 검사**
   - `훈몽자회` 등 사전류 말뭉치의 경우 DB `text_raw`의 앞부분에서 한자 표제어를 추출하여, 원본 XML의 `<g>{한자}</g>` 패턴과 정확히 일치하는 행을 탐색.
4. **4단계: 토큰 다중 어절 스코어링(Scoring) 매칭**
   - 문장 내에서 2음절 이상의 유효 단어 토큰들을 추출하고, 원본 파일의 각 행에 대해 일치하는 토큰 수(score)를 계산하여 가장 높은 점수를 획득한 행을 타깃 라인으로 선정.

#### (2) PC 기본 텍스트 에디터 자동 실행 엔진 (`engine/src/main.rs: open_in_external_editor`)
사용자의 컴퓨터 환경을 자동 감지하여 가장 적합한 에디터로 해당 파일과 라인을 즉시 엽니다:

1. **VS Code (Visual Studio Code)**:
   - `code -g "파일경로:라인번호"` 명령을 실행하여 원본 파일이 열림과 동시에 정확한 해당 라인으로 커서가 이동.
2. **Notepad++**:
   - `notepad++ -n라인번호 "파일경로"` 명령을 실행하여 Notepad++에서 즉시 해당 라인으로 열림.
3. **Windows 메모장 (notepad.exe) 및 기본 셸 연결**:
   - VS Code나 Notepad++가 없는 일반 Windows 환경에서는 `notepad.exe "파일경로"` 또는 `cmd /c start "" "파일경로"`로 원본 파일 즉시 실행.
4. **macOS / Linux**:
   - `open -e` 또는 `xdg-open`을 통해 데스크톱 기본 에디터로 열림.

#### (3) 백엔드 API 신설 (`/api/open-editor`) 및 프론트엔드 연동
- 백엔드에 `/api/open-editor?file_path={경로}&line={라인}` 엔드포인트를 추가하여 에디터 재실행 요청 처리.
- 검색 결과 행 우클릭 컨텍스트 메뉴 명칭을 **[원본 파일의 해당 위치로 점프 (PC 텍스트 에디터로 열기)]**로 명확화.
- 점프 모달 상단에 `🖥️ 에디터 연동: Visual Studio Code(라인 #11)로 파일을 열었습니다.` 형태의 상태 안내 뱃지 렌더링.
- 점프 모달 푸터에 **[🖥️ PC 텍스트 에디터로 열기]** 버튼(`#btn-reopen-editor`)을 추가하여 언제든 외부 에디터로 다시 열 수 있도록 지원.

---

## 4. 검증 결과

### 4.1 Rust 단위 테스트 (`cargo test`)
- 위치 추적 정밀도 및 에디터 연동 API 유효성에 대한 단위 테스트 100% 통과:
  - `test_find_target_line_xml_hunmong`: `훈몽자회.xml`의 '天' 항목 검색 시 정확히 **라인 #11** 적중 검증 완료.
  - `test_find_target_line_xml_sent`: `간이벽온방.xml`의 문장 검색 시 정확히 **라인 #10** 적중 검증 완료.
  - `test_find_target_line_sejong`: `BTAA0001.txt` 세종 말뭉치 문장 검색 시 정확히 **라인 #118** 적중 검증 완료.

### 4.2 자바스크립트 문법 검증
- `node -c web/app.js`: 문법 에러 0건, 정상 통과.

### 4.3 릴리스 바이너리 빌드
- `cargo build --release` 컴파일 완료.
- 최신 바이너리가 `c:\AG\concordance\sconc.exe`에 정상 배포 완료.

---

## 5. 변경 파일 목록

| 구분 | 파일 경로 | 주요 변경 내용 |
| :--- | :--- | :--- |
| **백엔드 (Rust)** | [`engine/src/main.rs`](file:///c:/AG/concordance/engine/src/main.rs) | `find_target_line_in_source` 4단계 탐색 알고리즘 구현, `open_in_external_editor` PC 에디터 프로세스 구동 함수 구현, `/api/open-editor` API 엔드포인트 신설, 단위 테스트 추가 |
| **마크업 (HTML)** | [`web/index.html`](file:///c:/AG/concordance/web/index.html) | 검색창 상단 폴딩 버튼(`#btn-toggle-search-options`) 추가, 우클릭 메뉴 텍스트 개선, 점프 모달 내 에디터 열기 버튼(`#btn-reopen-editor`) 추가 |
| **스타일 (CSS)** | [`web/styles.css`](file:///c:/AG/concordance/web/styles.css) | 전역 스크롤 허용(`overflow-y: auto`), 결과 테이블 최소 높이(`min-height: 360px~480px`) 보장, `.search-options-bar.collapsed` 스타일 적용 |
| **스크립트 (JS)** | [`web/app.js`](file:///c:/AG/concordance/web/app.js) | 상단 옵션 폴딩/펼치기 토글 및 `localStorage` 연동, 점프 시 에디터 상태 뱃지 렌더링, `#btn-reopen-editor` 클릭 이벤트 연결 |
| **배포 실행 파일** | [`sconc.exe`](file:///c:/AG/concordance/sconc.exe) | 최신 릴리스 바이너리 복사 및 배포 준비 완료 |
| **작업 일지** | [`WORKLOG.md`](file:///c:/AG/concordance/WORKLOG.md) | 8차 작업 내역 상세 기록 |
