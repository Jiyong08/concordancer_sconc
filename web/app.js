(() => {
  const automata = new window.HancomOldHangulAutomata();
  let oldHangulImeActive = false;
  let currentSegmentDetail = null;
  let editModeActive = false;
  let inspectorExpanded = false; // 기본값: 접힘 (Folded)
  let currentSearchAbort = null;
let currentSearchSeq = 0;
let searchExplicitlyCancelled = false;
let isSearchInProgress = false;

  const appShell = document.getElementById('app-shell');
  const selectAspectRatio = document.getElementById('select-aspect-ratio');

  const searchInput = document.getElementById('search-input');
  const btnSearch = document.getElementById('btn-search');
  const btnCancelSearch = document.getElementById('btn-cancel-search');
  const btnToggleOldIme = document.getElementById('btn-toggle-old-ime');
  const btnToggleJamoPad = document.getElementById('btn-toggle-jamo-pad');
  const btnClearInput = document.getElementById('btn-clear-input');
  const jamoMiniPad = document.getElementById('jamo-mini-pad');
  const kbStatus = document.getElementById('kb-automata-status');

  const selectMode = document.getElementById('select-mode');
  const btnSearchMethodTrigger = document.getElementById('btn-search-method-trigger');
  const modalSearchMethod = document.getElementById('modal-search-method');
  const btnCloseMethodModal = document.getElementById('btn-close-method-modal');
  const btnCancelMethodModal = document.getElementById('btn-cancel-method-modal');
  const btnApplyMethodModal = document.getElementById('btn-apply-method-modal');
  const searchMethodSummaryLabel = document.getElementById('search-method-summary-label');
  const methodModalSummaryText = document.getElementById('method-modal-summary-text');
  const btnMethodAll = document.getElementById('btn-method-all');

  const chkTargetSurface = document.getElementById('chk-target-surface');
  const chkTargetMorph = document.getElementById('chk-target-morph');
  const chkTargetSense = document.getElementById('chk-target-sense');
  const chkTargetSyntax = document.getElementById('chk-target-syntax');
  const chkTargetVector = document.getElementById('chk-target-vector');

  const chkMethodJamo = document.getElementById('chk-method-jamo');
  const chkMethodRegex = document.getElementById('chk-method-regex');

  const btnTargetAll = document.getElementById('btn-target-all');
  const btnTargetSurfaceOnly = document.getElementById('btn-target-surface-only');
  const selectLayer = document.getElementById('select-layer');
  const selectDoc = document.getElementById('select-doc');
  const selectContextWords = document.getElementById('select-context-words');
  const selectLimit = document.getElementById('select-limit');
  const selectCorpusFont = document.getElementById('select-corpus-font');

  // Search Options Collapsible Bar (상단 카드 헤더 + 검색창 줄 인라인 버튼)
  const btnToggleSearchOptions = document.getElementById('btn-toggle-search-options');
  const btnToggleOptionsInline = document.getElementById('btn-toggle-options-inline');
  const searchOptionsBar = document.getElementById('search-options-bar');
  const foldIcon = document.getElementById('fold-icon');
  const foldText = document.getElementById('fold-text');
  const inlineFoldIcon = document.getElementById('inline-fold-icon');
  const inlineFoldText = document.getElementById('inline-fold-text');

  // Corpus Document Selector Elements
  const btnOpenDocSelector = document.getElementById('btn-open-doc-selector');
  const docSelectorLabel = document.getElementById('doc-selector-label');
  const modalDocSelector = document.getElementById('modal-doc-selector');
  const btnCloseDocSelector = document.getElementById('btn-close-doc-selector');
  const btnCancelDocSelector = document.getElementById('btn-cancel-doc-selector');
  const btnApplyDocSelector = document.getElementById('btn-apply-doc-selector');
  const btnDocSelectAll = document.getElementById('btn-doc-select-all');
  const btnDocDeselectAll = document.getElementById('btn-doc-deselect-all');
  const btnDocExpandAll = document.getElementById('btn-doc-expand-all');
  const btnDocCollapseAll = document.getElementById('btn-doc-collapse-all');
  const docSearchFilter = document.getElementById('doc-search-filter');
  const docTreeContainer = document.getElementById('doc-tree-container');
  const docSelectedCount = document.getElementById('doc-selected-count');
  const docTotalCount = document.getElementById('doc-total-count');
  const docSelectedSentences = document.getElementById('doc-selected-sentences');

  // Search Unit & User Corpus Bundles Elements
  const selectSearchUnit = document.getElementById('select-search-unit');
  const btnSetDefaultSearchUnit = document.getElementById('btn-set-default-search-unit');
  const STORAGE_KEY_DEFAULT_SEARCH_UNIT = 'sconc_default_search_unit';

  const selectUserCorpus = document.getElementById('select-user-corpus');
  const btnCreateDocBundle = document.getElementById('btn-create-doc-bundle');
  const selectModalUserBundle = document.getElementById('select-modal-user-bundle');
  const btnDeleteModalUserBundle = document.getElementById('btn-delete-modal-user-bundle');
  let activeUserBundleName = null;
  const STORAGE_KEY_BUNDLES = 'sconc_user_corpus_bundles';

  let globalDocuments = [];
  let globalSourceFiles = [];
  let selectedDocIds = new Set(['ALL']); // 기본값: 전체 문서
  let tempSelectedDocIds = new Set();

  const kwicTbody = document.getElementById('kwic-tbody');
  const kwicResultTitle = document.getElementById('kwic-result-title');
  const kwicResultMeta = document.getElementById('kwic-result-meta');

  const inspectorFoldCard = document.getElementById('inspector-fold-card');
  const inspectorFoldHeader = document.getElementById('inspector-fold-header');
  const inspectorFoldArrow = document.getElementById('inspector-fold-arrow');
  const inspectorSelectedSummary = document.getElementById('inspector-selected-summary');
  const btnFoldToggleBtn = document.getElementById('btn-fold-toggle-btn');
  const inspectorBody = document.getElementById('inspector-body');
  const btnToggleEdit = document.getElementById('btn-toggle-edit');

  // Multi-Tab & Sub-search & Export & Jump Elements
  const kwicTabsBar = document.getElementById('kwic-tabs-bar');
  const subsearchInput = document.getElementById('subsearch-input');
  const btnSubsearch = document.getElementById('btn-subsearch');
  const btnSubsearchReset = document.getElementById('btn-subsearch-reset');
  const subsearchCountBadge = document.getElementById('subsearch-count-badge');
  const btnExportKwicTxt = document.getElementById('btn-export-kwic-txt');

  const kwicContextMenu = document.getElementById('kwic-context-menu');
  const ctxJump = document.getElementById('ctx-jump');
  const ctxDetail = document.getElementById('ctx-detail');
  const ctxCopy = document.getElementById('ctx-copy');

  const modalSourceJump = document.getElementById('modal-source-jump');
  const modalJumpTitle = document.getElementById('modal-jump-title');
  const modalJumpMeta = document.getElementById('modal-jump-meta');
  const modalJumpLines = document.getElementById('modal-jump-lines');
  const btnCloseJumpModal = document.getElementById('btn-close-jump-modal');
  const btnCloseJumpModalBottom = document.getElementById('btn-close-jump-modal-bottom');
  const btnCopyJumpText = document.getElementById('btn-copy-jump-text');
  const btnReopenEditor = document.getElementById('btn-reopen-editor');

  // DB Builder Elements
  const builderFolderPath = document.getElementById('builder-folder-path');
  const builderDbName = document.getElementById('builder-db-name');
  const btnStartBuild = document.getElementById('btn-start-build');
  const builderStatusBadge = document.getElementById('builder-status-badge');
  const builderResultArea = document.getElementById('builder-result-area');
  const builderSummaryBanner = document.getElementById('builder-summary-banner');
  const builderItemsTbody = document.getElementById('builder-items-tbody');
  const btnJumpToSearch = document.getElementById('btn-jump-to-search');

  // State Management
  let searchTabs = []; // Max 10 tabs: { id, query, title, mode, layer, docId, contextWords, limit, data, subsearchQuery }
  let activeTabId = null;
  let rightClickedSegId = null;
  let currentJumpLinesData = [];
  let currentJumpFilePath = '';
  let currentJumpLineNo = null;

  function escapeHtml(str) {
    if (!str) return '';
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  // 1. Monitor Aspect Ratio Detection & Switching (16:9 or 4:3)
  function applyAspectRatio() {
    const mode = selectAspectRatio.value;
    appShell.classList.remove('ratio-16-9', 'ratio-4-3', 'ratio-full');
    if (mode === '16:9') {
      appShell.classList.add('ratio-16-9');
    } else if (mode === '4:3') {
      appShell.classList.add('ratio-4-3');
    } else if (mode === 'full') {
      appShell.classList.add('ratio-full');
    } else {
      // auto: detect user's monitor aspect ratio
      const sw = window.screen.width || window.innerWidth;
      const sh = window.screen.height || window.innerHeight;
      const ratio = sw / sh;
      if (ratio >= 1.55) {
        appShell.classList.add('ratio-16-9');
      } else {
        appShell.classList.add('ratio-4-3');
      }
    }
  }

  selectAspectRatio.addEventListener('change', applyAspectRatio);
  window.addEventListener('resize', () => {
    if (selectAspectRatio.value === 'auto') applyAspectRatio();
  });
  applyAspectRatio();

  // 최상단 메뉴 바 마우스 휠로 가로 스크롤 지원 (14인치 등 작은 화면 편의 기능)
  const mainNavTabs = document.getElementById('main-nav-tabs');
  if (mainNavTabs) {
    mainNavTabs.addEventListener('wheel', (e) => {
      if (e.deltaY !== 0) {
        mainNavTabs.scrollLeft += e.deltaY;
        e.preventDefault();
      }
    }, { passive: false });
  }

  // 2. Corpus View Font Selector (기본 내장 KoPubWorld Dotum Medium.ttf + 사용자 선택 폰트)
  const FONT_MAP = {
    kopub: "'KoPubWorldDotum', 'HANBatang', sans-serif",
    hanbatang: "'HANBatang', 'KoPubWorldDotum', serif",
    malgun: "'Malgun Gothic', '맑은 고딕', 'HANBatang', sans-serif",
    batang: "'Batang', '바탕', 'NanumMyeongjo', 'HANBatang', serif",
  };

  selectCorpusFont.addEventListener('change', () => {
    const fontStack = FONT_MAP[selectCorpusFont.value] || FONT_MAP.kopub;
    document.documentElement.style.setProperty('--font-corpus', fontStack);
  });

  // Search Unit (어절 단위 / 문장 단위 / 텍스트 단위) & User-Selectable Default Persistence
  const SEARCH_UNIT_BASE_LABELS = {
    eojeol: '어절 단위 검색',
    sentence: '문장 단위 검색',
    text: '텍스트 단위 검색',
  };

  function getSavedDefaultSearchUnit() {
    try {
      const saved = localStorage.getItem(STORAGE_KEY_DEFAULT_SEARCH_UNIT);
      if (saved === 'eojeol' || saved === 'sentence' || saved === 'text') {
        return saved;
      }
    } catch (_) {}
    return 'eojeol';
  }

  function refreshSearchUnitOptionLabels() {
    if (!selectSearchUnit) return;
    const defUnit = getSavedDefaultSearchUnit();
    Array.from(selectSearchUnit.options).forEach((opt) => {
      const base = SEARCH_UNIT_BASE_LABELS[opt.value] || opt.textContent;
      opt.textContent = opt.value === defUnit ? `${base} (기본)` : base;
    });
    if (btnSetDefaultSearchUnit) {
      const isCurDefault = selectSearchUnit.value === defUnit;
      btnSetDefaultSearchUnit.textContent = isCurDefault ? '📌 기본값' : '📌 기본값 지정';
      btnSetDefaultSearchUnit.classList.toggle('is-default', isCurDefault);
      btnSetDefaultSearchUnit.title = isCurDefault
        ? `현재 선택된 '${SEARCH_UNIT_BASE_LABELS[defUnit]}'이(가) 기본값(디폴트)으로 저장되어 있습니다.`
        : `클릭 시 현재 선택된 '${SEARCH_UNIT_BASE_LABELS[selectSearchUnit.value]}'을(를) 기본값(디폴트)으로 저장합니다.`;
    }
  }

  if (selectSearchUnit) {
    selectSearchUnit.value = getSavedDefaultSearchUnit();
    refreshSearchUnitOptionLabels();
    selectSearchUnit.addEventListener('change', () => {
      refreshSearchUnitOptionLabels();
      runSearch();
    });
  }

  if (btnSetDefaultSearchUnit && selectSearchUnit) {
    btnSetDefaultSearchUnit.addEventListener('click', () => {
      const curUnit = selectSearchUnit.value || 'eojeol';
      try {
        localStorage.setItem(STORAGE_KEY_DEFAULT_SEARCH_UNIT, curUnit);
      } catch (_) {}
      refreshSearchUnitOptionLabels();
      btnSetDefaultSearchUnit.textContent = '✓ 기본값 저장됨';
      setTimeout(() => {
        refreshSearchUnitOptionLabels();
      }, 1200);
    });
  }

  // Re-run search when Context Words or Limit changes
  selectContextWords.addEventListener('change', () => runSearch());
  selectLimit.addEventListener('change', () => runSearch());

  // 3. Tab Switching
  document.querySelectorAll('.nav-tab').forEach((btn) => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.nav-tab').forEach((b) => b.classList.remove('active'));
      document.querySelectorAll('.tab-panel').forEach((p) => p.classList.remove('active'));
      btn.classList.add('active');
      const target = document.getElementById(btn.dataset.tab);
      if (target) target.classList.add('active');
    });
  });

  // 4. Collapsible (Folding) Multi-layer Inspector (기본값: 접힘)
  function setInspectorExpanded(expanded) {
    inspectorExpanded = expanded;
    if (expanded) {
      inspectorFoldCard.classList.add('expanded');
      inspectorFoldArrow.textContent = '▼';
      btnFoldToggleBtn.textContent = '접기 ▾';
    } else {
      inspectorFoldCard.classList.remove('expanded');
      inspectorFoldArrow.textContent = '▶';
      btnFoldToggleBtn.textContent = '펼치기 ▴';
    }
    updateInspectorHeaderSummary();
  }

  function updateInspectorHeaderSummary() {
    if (!currentSegmentDetail) {
      inspectorSelectedSummary.textContent = inspectorExpanded
        ? '— (문장을 선택해 주세요)'
        : '— (현재 접혀 있음 · 클릭하여 펼치기)';
      return;
    }
    const d = currentSegmentDetail;
    const loc = d.page_ref ? `${d.doc_id} (${d.page_ref})` : `${d.doc_id} #${d.seq_no}`;
    const preview = d.text_display.length > 38 ? d.text_display.slice(0, 38) + '…' : d.text_display;
    inspectorSelectedSummary.textContent = inspectorExpanded
      ? `— [${loc}] ${preview}`
      : `— 선택됨: [${loc}] "${preview}" (클릭하여 펼치기)`;
  }

  inspectorFoldHeader.addEventListener('click', () => {
    setInspectorExpanded(!inspectorExpanded);
  });
  btnFoldToggleBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    setInspectorExpanded(!inspectorExpanded);
  });

  // 5. Toggle Old Hangul IME (한컴 두벌식 옛글 오토마타)
  function setOldHangulIme(active) {
    oldHangulImeActive = active;
    if (active) {
      automata.reset();
      if (searchInput.value) {
        automata.feedRawText(searchInput.value);
      }
      btnToggleOldIme.classList.add('active');
      btnToggleOldIme.textContent = '⌨️ 옛한글 입력: ON';
      searchInput.classList.add('old-hangul-active');
      updateKbStatus();
    } else {
      btnToggleOldIme.classList.remove('active');
      btnToggleOldIme.textContent = '⌨️ 옛한글 입력: OFF';
      searchInput.classList.remove('old-hangul-active');
      kbStatus.textContent = '옛글 낱자 클릭:';
    }
    searchInput.focus();
  }

  function updateKbStatus() {
    if (!oldHangulImeActive) return;
    kbStatus.textContent = `🟣 ${automata.getCurrentStateSummary()}`;
  }

  btnToggleOldIme.addEventListener('click', () => {
    setOldHangulIme(!oldHangulImeActive);
  });

  btnToggleJamoPad.addEventListener('click', () => {
    jamoMiniPad.classList.toggle('open');
  });

  btnClearInput.addEventListener('click', () => {
    automata.reset();
    searchInput.value = '';
    updateKbStatus();
    searchInput.focus();
  });

  // Intercept keystrokes when Hancom Old Hangul IME is ON
  searchInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      if (e.isComposing || e.keyCode === 229) return;
      e.preventDefault();
      runSearch();
      return;
    }

    if (!oldHangulImeActive) return;
    if (e.ctrlKey || e.altKey || e.metaKey) return;

    if (e.code === 'Backspace') {
      e.preventDefault();
      automata.backspace();
      searchInput.value = automata.getText();
      updateKbStatus();
      return;
    }

    if (e.code === 'Space') {
      e.preventDefault();
      automata.feedSpace(e.shiftKey);
      searchInput.value = automata.getText();
      updateKbStatus();
      return;
    }

    const entry = window.KEY_LAYOUT[e.code];
    if (entry) {
      e.preventDefault();
      const target = e.shiftKey ? entry.shift : entry.base;
      const [kind, val] = target;
      if (kind === 'L') {
        automata.feedConsonant(val);
      } else if (kind === 'LL') {
        automata.feedDoubleConsonant(val);
      } else if (kind === 'L_SEQ') {
        val.forEach((p) => automata.feedConsonant(p));
      } else if (kind === 'V') {
        automata.feedVowel(val);
      } else if (kind === 'TONE') {
        automata.feedTone(val);
      } else if (kind === 'ALONE') {
        automata.feedStandaloneVowel(val);
      }
      searchInput.value = automata.getText();
      updateKbStatus();
      return;
    }

    // [옛한글 입력기 활성화 상태에서도 숫자, 영문, 문장부호, 정규식/자소 검색 기호 정상 입력]
    if (e.key && e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      automata.feedChar(e.key);
      searchInput.value = automata.getText();
      updateKbStatus();
      return;
    }
  });

  searchInput.addEventListener('input', () => {
    if (oldHangulImeActive) {
      // 외부 붙여넣기나 마우스 조작 등으로 내용이 달라진 경우 오토마타와 상태 동기화
      if (searchInput.value !== automata.getText()) {
        automata.reset();
        automata.feedRawText(searchInput.value);
      }
    }
  });

  // Mini Archaic Jamo Pad buttons
  document.querySelectorAll('.jamo-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      if (!oldHangulImeActive) {
        setOldHangulIme(true);
      }
      const t = btn.dataset.type;
      if (t === 'L') {
        automata.feedConsonant(btn.dataset.name);
      } else if (t === 'L_SEQ') {
        const parts = btn.dataset.seq.split(',');
        parts.forEach((p) => automata.feedConsonant(p));
      } else if (t === 'V') {
        automata.feedVowel(btn.dataset.name);
      } else if (t === 'V_SEQ') {
        const parts = btn.dataset.seq.split(',');
        parts.forEach((p) => automata.feedVowel(p));
      } else if (t === 'TONE') {
        automata.feedTone(Number(btn.dataset.level));
      }
      searchInput.value = automata.getText();
      updateKbStatus();
      searchInput.focus();
    });
  });

  // Preset Chips (Located in Usage Guide Tab)
  document.querySelectorAll('.preset-chip').forEach((chip) => {
    chip.addEventListener('click', () => {
      const q = chip.dataset.q || '';
      const mode = chip.dataset.mode || 'all';
      const layer = chip.dataset.layer || 'ALL';
      // Switch to Search Tab
      document.querySelector('.nav-tab[data-tab="tab-search"]').click();
      automata.reset();
      automata.feedRawText(q);
      searchInput.value = q;
      if (selectMode) selectMode.value = mode;
      if (selectLayer) selectLayer.value = layer;

      // Sync single-select target radio & method checkboxes with preset mode
      if (chkTargetSurface) chkTargetSurface.checked = (mode !== 'morph' && mode !== 'sense' && mode !== 'syntax' && mode !== 'vector');
      if (chkTargetMorph) chkTargetMorph.checked = (mode === 'morph');
      if (chkTargetSense) chkTargetSense.checked = (mode === 'sense');
      if (chkTargetSyntax) chkTargetSyntax.checked = (mode === 'syntax');
      if (chkTargetVector) chkTargetVector.checked = (mode === 'vector');
      if (chkMethodJamo) chkMethodJamo.checked = (mode === 'jamo' || (q.includes('[') && q.includes(']') && q.includes('/')));
      if (chkMethodRegex) chkMethodRegex.checked = (mode === 'regex');
      updateSearchMethodSummaryLabel();

      selectedDocIds = new Set(['ALL']);
      updateDocSelectorTriggerLabel();
      updateKbStatus();
      runSearch();
    });
  });

  let lastSearchClickTime = 0;
  btnSearch.addEventListener('click', () => {
    const now = Date.now();
    if (now - lastSearchClickTime < 350) return; // 더블클릭 등 350ms 이내 중복 클릭 차단
    lastSearchClickTime = now;
    runSearch();
  });

  // 6. Search Cancel Button (검색 종료)
  btnCancelSearch.addEventListener('click', async () => {
    searchExplicitlyCancelled = true;
    if (currentSearchAbort) {
      try {
        currentSearchAbort.abort();
      } catch (_) {}
      currentSearchAbort = null;
    }
    try {
      await fetch('/api/search/cancel', { method: 'POST' });
    } catch (_) {}
    btnCancelSearch.style.display = 'none';
    if (btnSearch) {
      btnSearch.disabled = false;
      btnSearch.innerHTML = '🔍 검색';
    }
    isSearchInProgress = false;
    kwicResultMeta.innerHTML = '<strong style="color:#dc2626;">⏹ 사용자에 의해 검색이 종료되었습니다.</strong>';
  });

  // Load Overview
  async function loadOverview() {
    try {
      const res = await fetch('/api/overview');
      const data = await res.json();

      const statsBadge = document.getElementById('badge-corpus-stats');
      statsBadge.innerHTML = `원본 <strong>${data.total_source_files}개</strong> → 통합 문서 <strong>${data.total_documents}개</strong> · <strong>${data.total_sentences.toLocaleString()}문장</strong> (${data.total_eojeols.toLocaleString()}어절)`;

      const mem = data.memory_profile;
      const memBadge = document.getElementById('badge-memory-tier');
      memBadge.innerHTML = `⚡ <strong>${escapeHtml(mem.tier_label)}</strong> (RAM ${mem.total_ram_gb}GB)`;

      globalDocuments = data.documents || [];
      globalSourceFiles = data.source_files || [];
      refreshUserCorpusSelects();
      if (selectLayer && selectLayer.options && selectLayer.options.length > 0) {
        selectLayer.options[0].textContent = `전체 말뭉치 (${data.total_sentences.toLocaleString()}문장)`;
      }
      updateDocSelectorTriggerLabel();

      const navTabMapping = document.getElementById('nav-tab-mapping');
      if (navTabMapping) {
        navTabMapping.textContent = `🗂️ 원본 파일 ↔ 데이터베이스 매칭 테이블 (${data.total_source_files || 0}종)`;
      }
      const navTabDocs = document.getElementById('nav-tab-docs');
      if (navTabDocs) {
        navTabDocs.textContent = `📚 구축 데이터베이스 문서 목록 (${data.total_documents || 0}종)`;
      }

      const mapTbody = document.getElementById('mapping-tbody');
      mapTbody.innerHTML = data.source_files
        .map(
          (f) => `
        <tr>
          <td style="font-weight:700;">${f.file_id}</td>
          <td style="font-weight:700; color:#1e40af;">${escapeHtml(f.file_name)}</td>
          <td><code>${escapeHtml(f.original_encoding)}</code></td>
          <td>${escapeHtml(f.text_format)}</td>
          <td><strong>${escapeHtml(f.corpus_type)}</strong></td>
          <td>${escapeHtml(f.raw_notation_desc)}</td>
          <td>${escapeHtml(f.analysis_notation_desc)}</td>
          <td>
            <span class="layer-badge badge-morph" style="font-size:0.75rem;">
              ${escapeHtml(f.linked_doc_id)}
            </span>
          </td>
          <td><code>${escapeHtml(f.target_db_file)}</code></td>
          <td style="text-align:right;">${f.sentence_count.toLocaleString()}</td>
          <td style="text-align:right;">${f.eojeol_count.toLocaleString()}</td>
        </tr>
      `
        )
        .join('');

      const docsListTitle = document.getElementById('docs-list-title');
      if (docsListTitle) {
        docsListTitle.textContent = `📚 구축된 통합 말뭉치 목록 (총 ${data.documents.length}개 말뭉치 · ${data.total_sentences.toLocaleString()}문장)`;
      }

      const docsTbody = document.getElementById('docs-tbody');
      docsTbody.innerHTML = data.documents
        .map(
          (d) => `
        <tr data-doc-id="${escapeHtml(d.doc_id)}">
          <td style="text-align:center;">
            <input type="checkbox" class="chk-doc-row" data-doc-id="${escapeHtml(d.doc_id)}" />
          </td>
          <td><code><strong>${escapeHtml(d.doc_id)}</strong></code></td>
          <td style="font-weight:700;">${escapeHtml(d.title)}</td>
          <td>${
            d.speaker_info
              ? `<div class="meta-cell-preview btn-view-doc-meta" data-doc="${escapeHtml(d.doc_id)}" title="클릭하여 전체 보기&#10;[화자 정보] ${escapeHtml(d.speaker_info)}">${escapeHtml(d.speaker_info)}</div>`
              : '<span class="meta-cell-empty">-</span>'
          }</td>
          <td>${
            d.other_meta
              ? `<div class="meta-cell-preview btn-view-doc-meta" data-doc="${escapeHtml(d.doc_id)}" title="클릭하여 전체 보기&#10;[기타 정보] ${escapeHtml(d.other_meta)}">${escapeHtml(d.other_meta)}</div>`
              : '<span class="meta-cell-empty">-</span>'
          }</td>
          <td>${escapeHtml(d.corpus_category)}</td>
          <td><span class="layer-badge badge-syntax">${escapeHtml(d.annotation_layers)}</span></td>
          <td><code>${escapeHtml(d.source_files_list)}</code></td>
          <td style="text-align:right; font-weight:700;">${d.sentence_count.toLocaleString()}</td>
          <td style="text-align:right;">${d.eojeol_count.toLocaleString()}</td>
          <td style="text-align:center; white-space:nowrap;">
            <div style="display:inline-flex; gap:4px;">
              <button type="button" class="btn btn-primary btn-browse-doc" data-doc="${escapeHtml(d.doc_id)}" style="padding:3px 8px; font-size:0.74rem;">
                📖 열람
              </button>
              <button type="button" class="btn btn-outline btn-rebuild-doc" data-doc="${escapeHtml(d.doc_id)}" title="이 말뭉치의 원본 파일을 바탕으로 다시 DB로 재구축" style="padding:3px 8px; font-size:0.74rem; border-color:#3b82f6; color:#1d4ed8;">
                🔄 다시 구축
              </button>
              <button type="button" class="btn btn-outline btn-delete-doc" data-doc="${escapeHtml(d.doc_id)}" title="이 말뭉치를 DB에서 삭제" style="padding:3px 8px; font-size:0.74rem; border-color:#ef4444; color:#dc2626;">
                🗑️ 삭제
              </button>
            </div>
          </td>
        </tr>
      `
        )
        .join('');

      // 메타데이터 셀 클릭 시 전체 화자 정보 / 기타 정보 모달 열기
      docsTbody.querySelectorAll('.btn-view-doc-meta').forEach((el) => {
        el.addEventListener('click', () => {
          const docId = el.dataset.doc;
          const doc = globalDocuments.find((x) => x.doc_id === docId);
          if (doc) openDocMetaModal(doc);
        });
      });

      // 이 문서 열람 버튼 이벤트
      docsTbody.querySelectorAll('.btn-browse-doc').forEach((b) => {
        b.addEventListener('click', () => {
          const docId = b.dataset.doc;
          document.querySelector('.nav-tab[data-tab="tab-search"]').click();
          searchInput.value = '';
          automata.reset();
          selectedDocIds = new Set([docId]);
          updateDocSelectorTriggerLabel();
          selectLayer.value = 'ALL';
          selectMode.value = 'all';
          runSearch();
        });
      });

      // 개별 말뭉치 삭제 버튼 이벤트
      docsTbody.querySelectorAll('.btn-delete-doc').forEach((b) => {
        b.addEventListener('click', () => {
          const docId = b.dataset.doc;
          const doc = globalDocuments.find(x => x.doc_id === docId);
          const title = doc ? doc.title : docId;
          const sents = doc ? (doc.sentence_count || 0).toLocaleString() : '';
          deleteDocuments([docId], `말뭉치 [${title}] (${docId})을(를) 데이터베이스에서 삭제하시겠습니까?\n\n※ 해당 말뭉치와 연계된 ${sents}문장이 DB에서 제거됩니다.\n(컴퓨터의 원본 텍스트 파일은 그대로 유지되므로 언제든 다시 구축할 수 있습니다)`);
        });
      });

      // 개별 말뭉치 다시 구축 버튼 이벤트
      docsTbody.querySelectorAll('.btn-rebuild-doc').forEach((b) => {
        b.addEventListener('click', () => {
          const docId = b.dataset.doc;
          const doc = globalDocuments.find(x => x.doc_id === docId);
          if (doc) {
            triggerRebuildForDoc(doc);
          }
        });
      });

      // [요청 1 반영] 문서 목록 체크박스 Shift 범위 다중선택 바인딩
      bindDocRowCheckboxEvents();
      updateSelectedDocsDeleteBtnCount();

      // [요청 반영 1] 프로그램 구동 시 자동 검색 비활성화 (사용자가 검색 버튼을 누를 때만 실행)
      searchInput.value = '';
      kwicResultMeta.textContent = '검색어를 입력하거나 특정 문서를 선택한 후 [🔍 검색]을 누르세요.';
    } catch (err) {
      console.error('Overview load error:', err);
    }
  }

  // ========================================================
  // Corpus Document Selector & Folder Tree Logic
  // ========================================================
  function getDocSelectionSummaryLabel() {
    const isAll = selectedDocIds.has('ALL') || selectedDocIds.size === globalDocuments.length || selectedDocIds.size === 0;
    if (isAll) {
      return `전체 문서 (${globalDocuments.length || 13}개)`;
    }
    if (selectedDocIds.size === 1) {
      const singleId = Array.from(selectedDocIds)[0];
      const doc = globalDocuments.find((d) => d.doc_id === singleId);
      return doc ? doc.title : `문서 [${singleId}]`;
    }
    return `${selectedDocIds.size}개 문서 선택됨`;
  }

  // =========================================================================
  // User Corpus Bundles (문서 묶음 & 사용자말뭉치) Management Logic
  // =========================================================================
  function getUserCorpusBundles() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY_BUNDLES);
      return raw ? JSON.parse(raw) : {};
    } catch (e) {
      return {};
    }
  }

  function saveUserCorpusBundle(name, docIds) {
    const bundles = getUserCorpusBundles();
    bundles[name] = {
      name: name,
      docIds: Array.from(docIds),
      updatedAt: Date.now(),
    };
    try {
      localStorage.setItem(STORAGE_KEY_BUNDLES, JSON.stringify(bundles));
    } catch (e) {
      console.error('Failed to save bundle to localStorage:', e);
    }
    refreshUserCorpusSelects();
  }

  function deleteUserCorpusBundle(name) {
    const bundles = getUserCorpusBundles();
    if (bundles[name]) {
      delete bundles[name];
      try {
        localStorage.setItem(STORAGE_KEY_BUNDLES, JSON.stringify(bundles));
      } catch (e) {}
      if (activeUserBundleName === name) {
        activeUserBundleName = null;
        selectedDocIds = new Set(['ALL']);
        updateDocSelectorTriggerLabel();
      }
      refreshUserCorpusSelects();
    }
  }

  function refreshUserCorpusSelects() {
    const bundles = getUserCorpusBundles();
    const names = Object.keys(bundles);

    // 1. 상단 옵션바 select-user-corpus
    if (selectUserCorpus) {
      const currentVal = activeUserBundleName || selectUserCorpus.value;
      selectUserCorpus.innerHTML = '<option value="">(전체 문서 / 미지정)</option>';
      names.forEach((name) => {
        const b = bundles[name];
        const count = (b.docIds || []).length;
        const opt = document.createElement('option');
        opt.value = name;
        opt.textContent = `📚 ${name} (${count}개 문서)`;
        if (name === currentVal) opt.selected = true;
        selectUserCorpus.appendChild(opt);
      });
      if (currentVal && names.includes(currentVal)) {
        selectUserCorpus.value = currentVal;
      } else {
        selectUserCorpus.value = '';
      }
    }

    // 2. 모달 내 select-modal-user-bundle
    if (selectModalUserBundle) {
      const currentVal = selectModalUserBundle.value;
      selectModalUserBundle.innerHTML = '<option value="">(불러올 묶음 선택...)</option>';
      names.forEach((name) => {
        const b = bundles[name];
        const count = (b.docIds || []).length;
        const opt = document.createElement('option');
        opt.value = name;
        opt.textContent = `${name} (${count}개)`;
        if (name === currentVal) opt.selected = true;
        selectModalUserBundle.appendChild(opt);
      });
      if (currentVal && names.includes(currentVal)) {
        selectModalUserBundle.value = currentVal;
      } else {
        selectModalUserBundle.value = '';
      }
    }
  }

  function updateDocSelectorTriggerLabel() {
    if (!docSelectorLabel) return;
    if (activeUserBundleName) {
      docSelectorLabel.textContent = `📂 사용자: ${activeUserBundleName} (${selectedDocIds.size}개)`;
      docSelectorLabel.style.color = '#7c3aed';
      return;
    }
    const isAll = selectedDocIds.has('ALL') || selectedDocIds.size === globalDocuments.length || selectedDocIds.size === 0;
    if (isAll) {
      const docCount = globalDocuments.length;
      docSelectorLabel.textContent = `📂 전체 문서 (${docCount}개 문서)`;
      docSelectorLabel.style.color = '#1e293b';
    } else {
      docSelectorLabel.textContent = `📂 ${selectedDocIds.size}개 문서 선택됨`;
      docSelectorLabel.style.color = '#2563eb';
    }
  }

  function buildFolderTreeData() {
    // Group files/docs by their directory path in rel_path
    const folderMap = new Map(); // folderName -> Array<{ doc, sourceFile }>

    globalDocuments.forEach((doc) => {
      // Find matching source_files for this doc
      const matchedFiles = globalSourceFiles.filter((sf) => sf.linked_doc_id === doc.doc_id);
      if (matchedFiles.length === 0) {
        // Fallback: use doc's source_files_list or category
        const relPath = doc.source_files_list || doc.title;
        const parts = relPath.replace(/\\/g, '/').split('/');
        const folderName = parts.length > 1 ? parts.slice(0, -1).join('/') : (doc.corpus_category || '기본 말뭉치');
        const fileName = parts[parts.length - 1];

        if (!folderMap.has(folderName)) folderMap.set(folderName, []);
        folderMap.get(folderName).push({
          doc,
          sourceFile: {
            file_id: doc.doc_id,
            file_name: fileName,
            rel_path: relPath,
            corpus_type: doc.corpus_category,
            file_size_bytes: 0,
          },
        });
      } else {
        matchedFiles.forEach((sf) => {
          const cleanRel = (sf.rel_path || sf.file_name).replace(/\\/g, '/');
          const parts = cleanRel.split('/');
          const folderName = parts.length > 1 ? parts.slice(0, -1).join('/') : (sf.corpus_type || doc.corpus_category || '기본 말뭉치');
          
          if (!folderMap.has(folderName)) folderMap.set(folderName, []);
          folderMap.get(folderName).push({
            doc,
            sourceFile: sf,
          });
        });
      }
    });

    return folderMap;
  }

  function renderDocTree(filterText = '') {
    if (!docTreeContainer) return;
    const folderMap = buildFolderTreeData();
    const filter = (filterText || '').trim().toLowerCase();

    let html = '';
    let totalFilesCount = 0;
    let selectedSentencesSum = 0;

    folderMap.forEach((items, folderName) => {
      const filteredItems = items.filter((item) => {
        if (!filter) return true;
        const txt = `${item.sourceFile.file_name} ${item.sourceFile.rel_path} ${item.doc.title} ${item.doc.corpus_category}`.toLowerCase();
        return txt.includes(filter);
      });

      if (filteredItems.length === 0) return;

      totalFilesCount += filteredItems.length;

      // Check folder selection state
      let checkedInFolder = 0;
      filteredItems.forEach((item) => {
        if (tempSelectedDocIds.has(item.doc.doc_id)) {
          checkedInFolder++;
        }
      });
      const allChecked = checkedInFolder === filteredItems.length;
      const someChecked = checkedInFolder > 0 && !allChecked;

      html += `
        <div class="doc-folder-group" data-folder="${escapeHtml(folderName)}">
          <div class="doc-folder-header">
            <div class="doc-folder-title-wrap">
              <span class="doc-folder-arrow">▼</span>
              <input type="checkbox" class="doc-folder-chk" data-folder="${escapeHtml(folderName)}" ${allChecked ? 'checked' : ''} ${someChecked ? 'data-indeterminate="true"' : ''} />
              <span style="font-size:0.92rem;">📁 <strong>${escapeHtml(folderName)}</strong></span>
            </div>
            <div class="doc-folder-stats">
              하위 파일 ${filteredItems.length}개
            </div>
          </div>
          <div class="doc-file-list">
      `;

      filteredItems.forEach((item) => {
        const isSelected = tempSelectedDocIds.has(item.doc.doc_id);
        if (isSelected) {
          selectedSentencesSum += item.doc.sentence_count || 0;
        }

        const badges = [];
        const layerText = item.doc.annotation_layers || '';
        if (item.doc.corpus_category && item.doc.corpus_category.includes('옛한글')) {
          badges.push('<span class="layer-badge badge-old" style="font-size:0.68rem; padding:1px 5px;">옛한글</span>');
        }
        if (layerText.includes('평문') || (layerText.includes('원문') && !layerText.includes('형태') && !layerText.includes('의미') && !layerText.includes('구문')) || layerText.includes('원문(')) {
          badges.push('<span class="layer-badge badge-old" style="font-size:0.68rem; padding:1px 5px;">평문</span>');
        }
        if (layerText.includes('형태')) {
          badges.push('<span class="layer-badge badge-morph" style="font-size:0.68rem; padding:1px 5px;">형태</span>');
        }
        if (layerText.includes('의미')) {
          badges.push('<span class="layer-badge badge-sense" style="font-size:0.68rem; padding:1px 5px;">의미</span>');
        }
        if (layerText.includes('구문')) {
          badges.push('<span class="layer-badge badge-syntax" style="font-size:0.68rem; padding:1px 5px;">구문</span>');
        }
        if (layerText.includes('벡터')) {
          badges.push('<span class="layer-badge badge-syntax" style="font-size:0.68rem; padding:1px 5px;">벡터</span>');
        }

        html += `
          <div class="doc-file-item ${isSelected ? 'selected' : ''}" data-doc-id="${escapeHtml(item.doc.doc_id)}">
            <div class="doc-file-info">
              <input type="checkbox" class="doc-file-chk" data-doc-id="${escapeHtml(item.doc.doc_id)}" data-folder="${escapeHtml(folderName)}" ${isSelected ? 'checked' : ''} />
              <span class="doc-file-name">${escapeHtml(item.sourceFile.file_name)}</span>
              <span class="doc-file-title" title="${escapeHtml(item.doc.title)}">${escapeHtml(item.doc.title)}</span>
              ${badges.join('')}
            </div>
            <div class="doc-file-meta">
              <span>${(item.doc.sentence_count || 0).toLocaleString()}문장</span>
              <span>·</span>
              <span>${(item.doc.eojeol_count || 0).toLocaleString()}어절</span>
            </div>
          </div>
        `;
      });

      html += `
          </div>
        </div>
      `;
    });

    if (totalFilesCount === 0) {
      docTreeContainer.innerHTML = '<div style="text-align:center; padding:30px; color:#64748b;">일치하는 문서나 파일이 없습니다.</div>';
    } else {
      docTreeContainer.innerHTML = html;
    }

    // Set indeterminate properties
    docTreeContainer.querySelectorAll('.doc-folder-chk[data-indeterminate="true"]').forEach((chk) => {
      chk.indeterminate = true;
    });

    // Update bottom counters
    updateDocModalCounters(selectedSentencesSum);

    // Bind event handlers
    attachDocTreeEvents();
  }

  function updateDocModalCounters(sentencesSum = null) {
    if (!docSelectedCount || !docTotalCount) return;
    const totalDocs = globalDocuments.length;
    let selectedDocsCount = 0;
    let sSum = 0;

    globalDocuments.forEach((d) => {
      if (tempSelectedDocIds.has(d.doc_id)) {
        selectedDocsCount++;
        sSum += d.sentence_count || 0;
      }
    });

    docSelectedCount.textContent = selectedDocsCount;
    docTotalCount.textContent = totalDocs;
    if (docSelectedSentences) {
      docSelectedSentences.textContent = (sentencesSum !== null ? sentencesSum : sSum).toLocaleString();
    }
  }

  function attachDocTreeEvents() {
    // 1. Folder collapse/expand arrow & header click
    docTreeContainer.querySelectorAll('.doc-folder-header').forEach((hdr) => {
      hdr.addEventListener('click', (e) => {
        if (e.target.classList.contains('doc-folder-chk')) return; // ignore checkbox click
        const group = hdr.closest('.doc-folder-group');
        const arrow = hdr.querySelector('.doc-folder-arrow');
        const list = group.querySelector('.doc-file-list');
        list.classList.toggle('collapsed');
        arrow.classList.toggle('collapsed');
      });
    });

    // 2. Folder checkbox change (select/deselect all children)
    docTreeContainer.querySelectorAll('.doc-folder-chk').forEach((chk) => {
      chk.addEventListener('change', (e) => {
        const isChecked = e.target.checked;
        const group = chk.closest('.doc-folder-group');
        group.querySelectorAll('.doc-file-chk').forEach((fChk) => {
          fChk.checked = isChecked;
          const docId = fChk.dataset.docId;
          if (isChecked) {
            tempSelectedDocIds.add(docId);
          } else {
            tempSelectedDocIds.delete(docId);
          }
          const itemEl = fChk.closest('.doc-file-item');
          if (itemEl) itemEl.classList.toggle('selected', isChecked);
        });
        chk.indeterminate = false;
        updateDocModalCounters();
      });
    });

    // 3. File checkbox change
    docTreeContainer.querySelectorAll('.doc-file-chk').forEach((chk) => {
      chk.addEventListener('change', (e) => {
        const isChecked = e.target.checked;
        const docId = chk.dataset.docId;
        const folderName = chk.dataset.folder;
        if (isChecked) {
          tempSelectedDocIds.add(docId);
        } else {
          tempSelectedDocIds.delete(docId);
        }
        const itemEl = chk.closest('.doc-file-item');
        if (itemEl) itemEl.classList.toggle('selected', isChecked);

        // Update parent folder checkbox
        const group = chk.closest('.doc-folder-group');
        if (group) {
          const folderChk = group.querySelector('.doc-folder-chk');
          const allFileChks = group.querySelectorAll('.doc-file-chk');
          const checkedFileChks = group.querySelectorAll('.doc-file-chk:checked');
          if (checkedFileChks.length === allFileChks.length) {
            folderChk.checked = true;
            folderChk.indeterminate = false;
          } else if (checkedFileChks.length > 0) {
            folderChk.checked = false;
            folderChk.indeterminate = true;
          } else {
            folderChk.checked = false;
            folderChk.indeterminate = false;
          }
        }

        updateDocModalCounters();
      });
    });

    // 4. File item row click (행 전체 클릭 시 체크박스 토글)
    docTreeContainer.querySelectorAll('.doc-file-item').forEach((item) => {
      item.addEventListener('click', (e) => {
        if (e.target.classList.contains('doc-file-chk') || e.target.type === 'checkbox') return;
        const chk = item.querySelector('.doc-file-chk');
        if (chk) {
          chk.checked = !chk.checked;
          chk.dispatchEvent(new Event('change'));
        }
      });
    });
  }

  function openDocSelectorModal() {
    if (!modalDocSelector) return;
    // Copy current selectedDocIds to tempSelectedDocIds
    tempSelectedDocIds = new Set();
    const isAll = selectedDocIds.has('ALL') || selectedDocIds.size === globalDocuments.length || selectedDocIds.size === 0;
    if (isAll) {
      globalDocuments.forEach((d) => tempSelectedDocIds.add(d.doc_id));
    } else {
      selectedDocIds.forEach((id) => tempSelectedDocIds.add(id));
    }

    if (docSearchFilter) docSearchFilter.value = '';
    renderDocTree();
    modalDocSelector.style.display = 'flex';
  }

  function closeDocSelectorModal() {
    if (modalDocSelector) {
      modalDocSelector.style.display = 'none';
    }
  }

  if (btnOpenDocSelector) {
    btnOpenDocSelector.addEventListener('click', openDocSelectorModal);
  }
  if (btnCloseDocSelector) {
    btnCloseDocSelector.addEventListener('click', closeDocSelectorModal);
  }
  if (btnCancelDocSelector) {
    btnCancelDocSelector.addEventListener('click', closeDocSelectorModal);
  }
  if (modalDocSelector) {
    modalDocSelector.addEventListener('click', (e) => {
      if (e.target === modalDocSelector) closeDocSelectorModal();
    });
  }

  if (btnDocSelectAll) {
    btnDocSelectAll.addEventListener('click', () => {
      globalDocuments.forEach((d) => tempSelectedDocIds.add(d.doc_id));
      renderDocTree(docSearchFilter ? docSearchFilter.value : '');
    });
  }

  if (btnDocDeselectAll) {
    btnDocDeselectAll.addEventListener('click', () => {
      tempSelectedDocIds.clear();
      renderDocTree(docSearchFilter ? docSearchFilter.value : '');
    });
  }

  if (btnDocExpandAll) {
    btnDocExpandAll.addEventListener('click', () => {
      docTreeContainer.querySelectorAll('.doc-file-list').forEach((l) => l.classList.remove('collapsed'));
      docTreeContainer.querySelectorAll('.doc-folder-arrow').forEach((a) => a.classList.remove('collapsed'));
    });
  }

  if (btnDocCollapseAll) {
    btnDocCollapseAll.addEventListener('click', () => {
      docTreeContainer.querySelectorAll('.doc-file-list').forEach((l) => l.classList.add('collapsed'));
      docTreeContainer.querySelectorAll('.doc-folder-arrow').forEach((a) => a.classList.add('collapsed'));
    });
  }

  if (docSearchFilter) {
    docSearchFilter.addEventListener('input', (e) => {
      renderDocTree(e.target.value);
    });
  }

  if (btnApplyDocSelector) {
    btnApplyDocSelector.addEventListener('click', () => {
      if (tempSelectedDocIds.size === 0) {
        alert('최소 1개 이상의 문서를 선택해 주세요.');
        return;
      }
      if (tempSelectedDocIds.size === globalDocuments.length) {
        selectedDocIds = new Set(['ALL']);
        activeUserBundleName = null;
        if (selectUserCorpus) selectUserCorpus.value = '';
      } else {
        selectedDocIds = new Set(tempSelectedDocIds);
        // Check if matches an existing bundle
        const bundles = getUserCorpusBundles();
        let matchedBundle = null;
        for (const bName of Object.keys(bundles)) {
          const bIds = new Set(bundles[bName].docIds || []);
          if (bIds.size === selectedDocIds.size && Array.from(selectedDocIds).every(id => bIds.has(id))) {
            matchedBundle = bName;
            break;
          }
        }
        activeUserBundleName = matchedBundle;
        if (selectUserCorpus) selectUserCorpus.value = matchedBundle || '';
      }
      updateDocSelectorTriggerLabel();
      closeDocSelectorModal();

      // 원시 말뭉치 선택 시 의미 주석 검색어 '세계__02' 자동 초기화 및 자동 검색 실행
      if (searchInput.value === '세계__02') {
        const hasSense = Array.from(selectedDocIds).some(id => {
          const doc = globalDocuments.find(d => d.doc_id === id);
          return doc && doc.annotation_layers && doc.annotation_layers.includes('의미');
        });
        if (!hasSense) {
          searchInput.value = '';
          automata.reset();
        }
      }
      runSearch();
    });
  }

  // User Corpus Bundle Events
  if (selectUserCorpus) {
    selectUserCorpus.addEventListener('change', () => {
      const val = selectUserCorpus.value;
      if (!val) {
        activeUserBundleName = null;
        selectedDocIds = new Set(['ALL']);
        updateDocSelectorTriggerLabel();
      } else {
        const bundles = getUserCorpusBundles();
        const b = bundles[val];
        if (b && b.docIds && b.docIds.length) {
          activeUserBundleName = val;
          selectedDocIds = new Set(b.docIds);
          updateDocSelectorTriggerLabel();
        }
      }
      if (selectModalUserBundle) selectModalUserBundle.value = val || '';
    });
  }

  if (btnCreateDocBundle) {
    btnCreateDocBundle.addEventListener('click', () => {
      if (tempSelectedDocIds.size === 0) {
        alert('현재 선택된 문서가 없습니다. 먼저 묶을 문서를 1개 이상 체크해 주세요.');
        return;
      }
      const name = prompt(`현재 선택된 ${tempSelectedDocIds.size}개 문서를 하나의 단위로 묶을 [사용자말뭉치] 이름을 입력하세요:`, '새말뭉치');
      if (!name || !name.trim()) return;
      const trimmed = name.trim();
      saveUserCorpusBundle(trimmed, tempSelectedDocIds);
      activeUserBundleName = trimmed;
      refreshUserCorpusSelects();
      if (selectModalUserBundle) selectModalUserBundle.value = trimmed;
      if (selectUserCorpus) selectUserCorpus.value = trimmed;
      alert(`문서 묶음 '${trimmed}' (${tempSelectedDocIds.size}개 문서)이 저장되었습니다!
이제 상단 [사용자말뭉치] 메뉴에서 언제든지 선택할 수 있습니다.`);
    });
  }

  if (selectModalUserBundle) {
    selectModalUserBundle.addEventListener('change', () => {
      const val = selectModalUserBundle.value;
      if (!val) return;
      const bundles = getUserCorpusBundles();
      const b = bundles[val];
      if (b && b.docIds) {
        tempSelectedDocIds = new Set(b.docIds);
        renderDocTree(docSearchFilter ? docSearchFilter.value : '');
        activeUserBundleName = val;
        if (selectUserCorpus) selectUserCorpus.value = val;
      }
    });
  }

  if (btnDeleteModalUserBundle) {
    btnDeleteModalUserBundle.addEventListener('click', () => {
      const val = selectModalUserBundle ? selectModalUserBundle.value : '';
      if (!val) {
        alert('삭제할 사용자말뭉치 묶음을 먼저 선택해 주세요.');
        return;
      }
      if (confirm(`사용자말뭉치 묶음 '${val}'을(를) 정말 삭제하시겠습니까?`)) {
        deleteUserCorpusBundle(val);
        alert(`'${val}' 묶음이 삭제되었습니다.`);
      }
    });
  }


  // Multi-Tab Management (최대 10개 탭 전환 및 비교)
  function renderTabsBar() {
    if (!kwicTabsBar) return;
    kwicTabsBar.innerHTML = searchTabs
      .map((tab) => {
        const isActive = tab.id === activeTabId;
        const total = tab.data && tab.data.total_matches ? tab.data.total_matches.toLocaleString() : 0;
        return `
          <div class="kwic-tab ${isActive ? 'active' : ''}" data-tab-id="${tab.id}" title="${escapeHtml(tab.title)} (${total}건)">
            <span>${escapeHtml(tab.title)} <small style="font-weight:400; opacity:0.85;">(${total})</small></span>
            <span class="kwic-tab-close" data-close-id="${tab.id}" title="탭 닫기">×</span>
          </div>
        `;
      })
      .join('');

    kwicTabsBar.innerHTML += `
      <div class="kwic-tab kwic-tab-add" id="btn-add-search-tab" title="새로운 검색 탭 열기" style="background:#e0e7ff; color:#3730a3; font-weight:700; border:1px dashed #6366f1;">
        + 새 검색
      </div>
    `;

    const btnAddTab = document.getElementById('btn-add-search-tab');
    if (btnAddTab) {
      btnAddTab.addEventListener('click', () => {
        searchInput.value = '';
        automata.reset();
        searchInput.focus();
        runSearch();
      });
    }

    kwicTabsBar.querySelectorAll('.kwic-tab:not(.kwic-tab-add)').forEach((el) => {
      el.addEventListener('click', (e) => {
        if (e.target.classList.contains('kwic-tab-close')) return;
        switchTab(el.dataset.tabId);
      });
    });

    kwicTabsBar.querySelectorAll('.kwic-tab-close').forEach((closeBtn) => {
      closeBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        closeTab(closeBtn.dataset.closeId);
      });
    });
  }

  function switchTab(tabId) {
    const tab = searchTabs.find((t) => t.id === tabId);
    if (!tab) return;
    activeTabId = tabId;

    // Restore inputs to match tab search settings
    searchInput.value = tab.query;
    automata.reset();
    automata.feedRawText(tab.query);
    if (selectMode) selectMode.value = tab.mode;
    if (selectLayer) selectLayer.value = tab.layer;
    if (selectSearchUnit && tab.searchUnit) {
      selectSearchUnit.value = tab.searchUnit;
      refreshSearchUnitOptionLabels();
    }
    if (tab.targets) {
      if (chkTargetSurface) chkTargetSurface.checked = (tab.targets === 'surface');
      if (chkTargetMorph) chkTargetMorph.checked = (tab.targets === 'morph');
      if (chkTargetSense) chkTargetSense.checked = (tab.targets === 'sense');
      if (chkTargetSyntax) chkTargetSyntax.checked = (tab.targets === 'syntax');
      if (chkTargetVector) chkTargetVector.checked = (tab.targets === 'vector');
      updateSearchMethodSummaryLabel();
    }
    if (tab.docIds) {
      selectedDocIds = new Set(tab.docIds);
    } else if (tab.docId && tab.docId !== 'ALL') {
      selectedDocIds = new Set([tab.docId]);
    } else {
      selectedDocIds = new Set(['ALL']);
    }
    updateDocSelectorTriggerLabel();
    selectContextWords.value = tab.contextWords;
    selectLimit.value = tab.limit;
    subsearchInput.value = tab.subsearchQuery || '';

    renderTabsBar();
    renderActiveTabHits();
  }

  function closeTab(tabId) {
    const idx = searchTabs.findIndex((t) => t.id === tabId);
    if (idx === -1) return;
    searchTabs.splice(idx, 1);
    if (activeTabId === tabId) {
      if (searchTabs.length > 0) {
        const nextTab = searchTabs[Math.max(0, idx - 1)];
        activeTabId = nextTab.id;
        switchTab(activeTabId);
      } else {
        activeTabId = null;
        renderTabsBar();
        kwicTbody.innerHTML = `
          <tr>
            <td colspan="5" style="text-align:center; padding:36px; color:#64748b;">
              모든 검색 탭이 닫혔습니다. 상단 검색창에서 새로운 검색을 실행하세요.
            </td>
          </tr>
        `;
        kwicResultTitle.textContent = '용례 검색 결과';
        kwicResultMeta.textContent = '검색어를 입력하세요.';
      }
    } else {
      renderTabsBar();
    }
  }

  // Active Tab Hits Rendering with Sub-search Filter
  function getFilteredHits(tab) {
    if (!tab || !tab.data || !tab.data.hits) return [];
    const subQ = (tab.subsearchQuery || '').trim().toLowerCase();
    if (!subQ) return tab.data.hits;

    return tab.data.hits.filter((h) => {
      const targetStr = [
        h.left_context,
        h.node,
        h.right_context,
        h.text_raw,
        h.text_unicode,
        h.doc_id,
        h.doc_title,
        h.morph_flat,
        h.sense_flat,
        h.syntax_tree,
        h.page_ref,
      ]
        .filter(Boolean)
        .join(' ')
        .toLowerCase();
      return targetStr.includes(subQ);
    });
  }

  function renderActiveTabHits() {
    const tab = searchTabs.find((t) => t.id === activeTabId);
    if (!tab || !tab.data) return;

    const data = tab.data;
    const ctxLabel = tab.contextWords === '999' ? '문장 전체' : `좌우 ${tab.contextWords}어절`;
    let docLabel = '전체 문서';
    if (tab.docIds && !tab.docIds.has('ALL') && tab.docIds.size > 0) {
      docLabel = tab.docIds.size === 1 ? Array.from(tab.docIds)[0] : `${tab.docIds.size}개 문서`;
    } else if (tab.docId && tab.docId !== 'ALL') {
      docLabel = tab.docId;
    }

    kwicResultTitle.textContent = tab.query
      ? `용례 검색 결과: "${tab.query}" [${docLabel}] (${ctxLabel})`
      : `문서 열람 (${docLabel} · ${ctxLabel})`;

    const cancelNote = data.cancelled
      ? ' · <strong style="color:#dc2626;">[중단됨: 부분 결과]</strong>'
      : '';
    const limitNote = tab.limit === '0' ? '무제한' : `최대 ${Number(tab.limit).toLocaleString()}건`;

    const visibleHits = getFilteredHits(tab);

    if (tab.subsearchQuery) {
      subsearchResetBtnState(true, data.hits.length, visibleHits.length);
      kwicResultMeta.innerHTML = `총 <strong>${data.total_matches.toLocaleString()}건</strong> 중 <strong>${visibleHits.length.toLocaleString()}건</strong> 필터링됨 (원래 표시 ${data.returned_count.toLocaleString()}건) · 소요시간 <strong>${data.elapsed_ms}ms</strong>${cancelNote}`;
    } else {
      subsearchResetBtnState(false, 0, 0);
      kwicResultMeta.innerHTML = `총 <strong>${data.total_matches.toLocaleString()}건</strong> 매칭 (표시 ${data.returned_count.toLocaleString()}건 / ${limitNote}) · 소요시간 <strong>${data.elapsed_ms}ms</strong>${cancelNote}`;
    }

    if (!visibleHits || visibleHits.length === 0) {
      kwicTbody.innerHTML = `
        <tr>
          <td colspan="5" style="text-align:center; padding:36px; color:#64748b;">
            ${tab.subsearchQuery ? `결과 내 재검색어 "<strong>${escapeHtml(tab.subsearchQuery)}</strong>"에 일치하는 결과가 없습니다.` : '조건에 일치하는 문장이 없습니다.'}
          </td>
        </tr>
      `;
      return;
    }

    kwicTbody.innerHTML = visibleHits.map((h) => createKwicRowHtml(h)).join('');

    const rows = kwicTbody.querySelectorAll('.kwic-row');
    rows.forEach((r) => {
      r.addEventListener('click', () => {
        rows.forEach((x) => x.classList.remove('selected'));
        r.classList.add('selected');
        tab.selectedSegId = r.dataset.segId;
        loadSegmentDetail(r.dataset.segId);
      });
      r.addEventListener('dblclick', () => {
        setInspectorExpanded(true);
      });
      // 마우스 우클릭 컨텍스트 메뉴 이벤트
      r.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        rows.forEach((x) => x.classList.remove('selected'));
        r.classList.add('selected');
        rightClickedSegId = r.dataset.segId;
        showContextMenu(e.clientX, e.clientY);
      });
    });

    if (rows.length > 0) {
      const targetSeg = tab.selectedSegId
        ? Array.from(rows).find((row) => row.dataset.segId == tab.selectedSegId) || rows[0]
        : rows[0];
      targetSeg.classList.add('selected');
      loadSegmentDetail(targetSeg.dataset.segId);
    }
  }

  function subsearchResetBtnState(hasFilter, total, filtered) {
    if (hasFilter) {
      btnSubsearchReset.style.display = 'inline-flex';
      subsearchCountBadge.style.display = 'inline-block';
      subsearchCountBadge.textContent = `${total}건 중 ${filtered}건 일치`;
    } else {
      btnSubsearchReset.style.display = 'none';
      subsearchCountBadge.style.display = 'none';
    }
  }

  // 4. Sub-Search (결과 내 재검색)
  function applySubsearch() {
    const tab = searchTabs.find((t) => t.id === activeTabId);
    if (!tab) return;
    tab.subsearchQuery = subsearchInput.value.trim();
    renderActiveTabHits();
  }

  function resetSubsearch() {
    const tab = searchTabs.find((t) => t.id === activeTabId);
    if (!tab) return;
    tab.subsearchQuery = '';
    subsearchInput.value = '';
    renderActiveTabHits();
  }

  btnSubsearch.addEventListener('click', applySubsearch);
  subsearchInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      applySubsearch();
    }
  });
  btnSubsearchReset.addEventListener('click', resetSubsearch);

  // 3. Search Result Export (사용자 지정 위치·파일명으로 텍스트 파일 저장)
  btnExportKwicTxt.addEventListener('click', async () => {
    const tab = searchTabs.find((t) => t.id === activeTabId);
    if (!tab || !tab.data || !tab.data.hits) {
      alert('저장할 검색 결과가 없습니다.');
      return;
    }
    const hits = getFilteredHits(tab);
    if (hits.length === 0) {
      alert('현재 표시된 검색 결과가 없습니다.');
      return;
    }

    const headers = [
      'No',
      '문서ID',
      '문서명',
      '문헌면수/순번',
      '좌측문맥(LeftContext)',
      '핵심어(Node)',
      '우측문맥(RightContext)',
      '일치층위',
      '평문',
      '형태',
      '의미',
      '구문',
    ];

    const lines = [headers.join('\t')];
    hits.forEach((h, idx) => {
      const loc = h.page_ref || `#${h.seq_no}`;
      lines.push(
        [
          idx + 1,
          h.doc_id,
          h.doc_title,
          loc,
          h.left_context,
          h.node,
          h.right_context,
          h.match_layer,
          h.text_unicode,
          h.morph_flat || '',
          h.sense_flat || '',
          h.syntax_tree || '',
        ]
          .map((v) => String(v).replace(/[\r\n\t]+/g, ' '))
          .join('\t')
      );
    });

    const fileContent = '\uFEFF' + lines.join('\r\n');
    const safeTitle = (tab.title || 'results').replace(/[\\/:*?"<>|]/g, '_');
    const dateStr = new Date().toISOString().slice(0, 10);
    const defaultName = `concordance_${safeTitle}_${dateStr}.txt`;

    try {
      btnExportKwicTxt.disabled = true;
      btnExportKwicTxt.textContent = '💾 저장 위치 선택 중...';
      const res = await fetch('/api/export/save-txt', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ default_name: defaultName, content: fileContent }),
      });
      const data = await res.json();
      if (data.ok && data.saved_path) {
        alert(`✅ 검색 결과(${hits.length}건)가 저장되었습니다.\n\n저장 위치: ${data.saved_path}`);
      } else if (data.error) {
        alert(`저장 오류: ${data.error}`);
      }
    } catch (err) {
      alert(`TXT 저장 요청 실패: ${err.message}`);
    } finally {
      btnExportKwicTxt.disabled = false;
      btnExportKwicTxt.textContent = '💾 결과 TXT 저장';
    }
  });

  // 6. Right-click Context Menu & Source Jump
  function showContextMenu(x, y) {
    if (!kwicContextMenu) return;
    kwicContextMenu.style.display = 'block';
    const menuWidth = 260;
    const menuHeight = 110;
    const posX = x + menuWidth > window.innerWidth ? window.innerWidth - menuWidth - 10 : x;
    const posY = y + menuHeight > window.innerHeight ? window.innerHeight - menuHeight - 10 : y;
    kwicContextMenu.style.left = `${posX}px`;
    kwicContextMenu.style.top = `${posY}px`;
  }

  function hideContextMenu() {
    if (kwicContextMenu) {
      kwicContextMenu.style.display = 'none';
    }
  }

  document.addEventListener('click', () => hideContextMenu());

  ctxJump.addEventListener('click', () => {
    hideContextMenu();
    if (rightClickedSegId) {
      openSourceJump(rightClickedSegId);
    }
  });

  ctxDetail.addEventListener('click', () => {
    hideContextMenu();
    if (rightClickedSegId) {
      loadSegmentDetail(rightClickedSegId);
      setInspectorExpanded(true);
    }
  });

  ctxCopy.addEventListener('click', () => {
    hideContextMenu();
    if (rightClickedSegId) {
      const row = kwicTbody.querySelector(`.kwic-row[data-seg-id="${rightClickedSegId}"]`);
      if (row && row.dataset.text) {
        navigator.clipboard.writeText(row.dataset.text).then(() => {
          alert('문장 텍스트가 클립보드에 복사되었습니다.');
        });
      }
    }
  });

  async function openSourceJump(segId) {
    if (!modalSourceJump) return;
    modalSourceJump.style.display = 'flex';
    modalJumpTitle.textContent = `📄 원본 텍스트 파일 위치 점프 (Segment #${segId})`;
    modalJumpMeta.innerHTML = '⏳ 원본 파일 위치 탐색 중…';
    modalJumpLines.innerHTML = '<div style="text-align:center; padding:30px; color:#64748b;">파일을 읽고 해당 문장의 원문 위치를 추적 중입니다…</div>';

    try {
      const res = await fetch(`/api/source-jump?segment_id=${encodeURIComponent(segId)}`);
      const data = await res.json();
      if (data.error) {
        modalJumpMeta.innerHTML = `<span style="color:#dc2626;">오류: ${escapeHtml(data.error)}</span>`;
        return;
      }

      currentJumpLinesData = data.lines || [];
      currentJumpFilePath = data.file_path || '';
      currentJumpLineNo = data.line_no || null;
      const lineNote = data.line_no ? ` · <strong>원문 라인 #${data.line_no}</strong>` : '';
      const pageNote = data.page_ref ? ` · 장차(Page): <code>${escapeHtml(data.page_ref)}</code>` : '';
      const sentNote = data.sent_id ? ` · 문장ID: <code>${escapeHtml(data.sent_id)}</code>` : '';
      const editorNote = data.editor_status 
        ? `<div style="margin-top:6px; padding:4px 10px; border-radius:4px; font-size:0.75rem; background:#eff6ff; color:#1e40af; border:1px solid #bfdbfe; display:inline-block;">🖥️ <strong>에디터 연동:</strong> ${escapeHtml(data.editor_status)}</div>`
        : '';

      modalJumpTitle.textContent = `📄 [${escapeHtml(data.doc_title)}] 원문 소스 점프`;
      modalJumpMeta.innerHTML = `
        <div><strong>원본 파일:</strong> <code>${escapeHtml(data.file_path)}</code> (${escapeHtml(data.file_name)})${lineNote}${pageNote}${sentNote}</div>
        <div style="margin-top:2px; font-size:0.75rem; color:#64748b;">원문 표기: <span class="corpus-text">${escapeHtml(data.text_raw || data.text_unicode)}</span></div>
        ${editorNote}
      `;

      if (!data.lines || data.lines.length === 0) {
        modalJumpLines.innerHTML = '<div style="padding:20px; text-align:center; color:#64748b;">표시할 라인이 없습니다.</div>';
        return;
      }

      modalJumpLines.innerHTML = data.lines
        .map(
          (line) => `
        <div class="jump-line ${line.is_target ? 'target' : ''}" id="jump-l-${line.line_no}">
          <span class="jump-line-num">${line.line_no}</span>
          <span class="jump-line-text">${escapeHtml(line.content)}</span>
        </div>
      `
        )
        .join('');

      // Scroll target line into center of view
      setTimeout(() => {
        const targetEl = modalJumpLines.querySelector('.jump-line.target');
        if (targetEl) {
          targetEl.scrollIntoView({ block: 'center', behavior: 'smooth' });
        }
      }, 50);
    } catch (err) {
      modalJumpMeta.innerHTML = `<span style="color:#dc2626;">위치 탐색 오류: ${err.message}</span>`;
    }
  }

  function closeJumpModal() {
    if (modalSourceJump) {
      modalSourceJump.style.display = 'none';
    }
  }

  btnCloseJumpModal.addEventListener('click', closeJumpModal);
  btnCloseJumpModalBottom.addEventListener('click', closeJumpModal);
  modalSourceJump.addEventListener('click', (e) => {
    if (e.target === modalSourceJump) closeJumpModal();
  });

  btnCopyJumpText.addEventListener('click', () => {
    if (!currentJumpLinesData || currentJumpLinesData.length === 0) return;
    const txt = currentJumpLinesData.map((l) => `${l.line_no}: ${l.content}`).join('\r\n');
    navigator.clipboard.writeText(txt).then(() => {
      alert('점프 뷰어 영역의 텍스트가 클립보드에 복사되었습니다.');
    });
  });

  // Open in PC Default Editor
  if (btnReopenEditor) {
    btnReopenEditor.addEventListener('click', async () => {
      if (!currentJumpFilePath) {
        alert('열 수 있는 원본 파일 경로가 없습니다.');
        return;
      }
      try {
        btnReopenEditor.disabled = true;
        btnReopenEditor.textContent = '⏳ 에디터 실행 중...';
        const url = `/api/open-editor?file_path=${encodeURIComponent(currentJumpFilePath)}${currentJumpLineNo ? `&line=${encodeURIComponent(currentJumpLineNo)}` : ''}`;
        const res = await fetch(url);
        const data = await res.json();
        if (data.error) {
          alert(`에디터 열기 실패: ${data.error}`);
        }
      } catch (err) {
        alert(`에디터 실행 요청 실패: ${err.message}`);
      } finally {
        btnReopenEditor.disabled = false;
        btnReopenEditor.textContent = '🖥️ PC 텍스트 에디터로 열기';
      }
    });
  }

  // ========================================================
  // Full Top Collapse (맨위 헤더 + 탭 + 검색 옵션 바 일괄 접기/펼치기)
  // 검색창 가장 우측 버튼(#btn-toggle-options-inline) 하나로 통합 제어
  // ========================================================
  const applyTopCollapse = (collapsed) => {
    if (!appShell) return;
    if (collapsed) {
      appShell.classList.add('top-collapsed');
      if (searchOptionsBar) searchOptionsBar.classList.add('collapsed');
      if (inlineFoldIcon) inlineFoldIcon.textContent = '🔽';
      if (inlineFoldText) inlineFoldText.textContent = '검색 옵션 펼치기';
      if (btnToggleOptionsInline) {
        btnToggleOptionsInline.classList.add('active');
        btnToggleOptionsInline.title = '맨 위의 상단 헤더와 검색 옵션을 다시 펼칩니다.';
      }
    } else {
      appShell.classList.remove('top-collapsed');
      if (searchOptionsBar) searchOptionsBar.classList.remove('collapsed');
      if (inlineFoldIcon) inlineFoldIcon.textContent = '🔼';
      if (inlineFoldText) inlineFoldText.textContent = '검색 옵션 접기';
      if (btnToggleOptionsInline) {
        btnToggleOptionsInline.classList.remove('active');
        btnToggleOptionsInline.title = '맨 위의 상단 헤더와 검색 옵션을 모두 접어 화면을 최대화합니다.';
      }
    }
  };

  const toggleTopCollapse = () => {
    if (!appShell) return;
    const isCurrentlyCollapsed = appShell.classList.contains('top-collapsed');
    const nextCollapsed = !isCurrentlyCollapsed;
    applyTopCollapse(nextCollapsed);
    if (nextCollapsed) {
      localStorage.setItem('sconc_top_collapsed', '1');
    } else {
      localStorage.removeItem('sconc_top_collapsed');
    }
  };

  if (btnToggleOptionsInline) {
    btnToggleOptionsInline.addEventListener('click', toggleTopCollapse);
  }

  // Restore saved state from localStorage
  if (localStorage.getItem('sconc_top_collapsed') === '1') {
    applyTopCollapse(true);
  }

  // =========================================================================
  // [요청 2, 3 반영] 말뭉치 폴더 지정 DB 생성 실시간 진행률 (%) 비동기 폴링 & UI
  // =========================================================================
  const builderProgressCard = document.getElementById('builder-progress-card');
  const builderProgressPct = document.getElementById('builder-progress-pct');
  const builderProgressBar = document.getElementById('builder-progress-bar');
  const builderProgressCurrentFile = document.getElementById('builder-progress-current-file');
  const builderProgressStats = document.getElementById('builder-progress-stats');
  const builderProgressLog = document.getElementById('builder-progress-log');
  const btnStopBuild = document.getElementById('btn-stop-build');
  const globalBuildTracker = document.getElementById('global-build-tracker');
  const globalTrackerText = document.getElementById('global-tracker-text');
  const globalTrackerBar = document.getElementById('global-tracker-bar');

  let buildPollingTimer = null;

  function stopBuildPolling() {
    if (buildPollingTimer) {
      clearInterval(buildPollingTimer);
      buildPollingTimer = null;
    }
  }

  // 상단 글로벌 트래커 클릭 시 DB 생성 탭으로 이동
  if (globalBuildTracker) {
    globalBuildTracker.addEventListener('click', () => {
      const tabBtn = document.querySelector('.nav-tab[data-tab="tab-build-db"]');
      if (tabBtn) tabBtn.click();
    });
  }

  // 중단 버튼 클릭
  if (btnStopBuild) {
    btnStopBuild.addEventListener('click', async () => {
      if (!confirm('현재 진행 중인 DB 생성을 중단하시겠습니까?\n(현재까지 변환된 문서는 DB에 보존됩니다)')) return;
      try {
        btnStopBuild.disabled = true;
        const res = await fetch('/api/build-db/stop', { method: 'POST' });
        const data = await res.json();
        alert(data.message || 'DB 생성이 중단되었습니다.');
      } catch (e) {
        alert('중단 요청 실패: ' + e.message);
      } finally {
        btnStopBuild.disabled = false;
      }
    });
  }

  function startBuildPolling() {
    stopBuildPolling();
    btnStartBuild.disabled = true;
    if (builderProgressCard) builderProgressCard.style.display = 'block';
    if (builderResultArea) builderResultArea.style.display = 'none';
    if (globalBuildTracker) globalBuildTracker.style.display = 'flex';

    buildPollingTimer = setInterval(async () => {
      try {
        const res = await fetch('/api/build-db/status');
        if (!res.ok) return;
        const prog = await res.json();

        // 1. 진행률 % 및 게이지 업데이트
        const pct = typeof prog.percent === 'number' ? Math.min(Math.max(prog.percent, 0), 100) : 0;
        const pctStr = pct.toFixed(1) + '%';
        if (builderProgressPct) builderProgressPct.textContent = pctStr;
        if (builderProgressBar) builderProgressBar.style.width = pctStr;

        // 상단 전역 배지 업데이트
        if (globalTrackerText) {
          const shortFile = prog.current_file ? ` (${prog.current_file.slice(0, 15)})` : '';
          globalTrackerText.textContent = `DB 구축 중: ${pctStr}${shortFile}`;
        }
        if (globalTrackerBar) globalTrackerBar.style.width = pctStr;

        // 2. 현재 파일명 및 세부 통계 업데이트
        if (builderProgressCurrentFile) {
          builderProgressCurrentFile.textContent = prog.current_file ? `현재 처리 중: ${prog.current_file}` : '폴더 내 파일 스캔 중...';
        }
        if (builderProgressStats) {
          const sents = (prog.sentences_imported || 0).toLocaleString();
          const eojeols = (prog.eojeols_imported || 0).toLocaleString();
          builderProgressStats.textContent = `파일: ${prog.processed_files || 0} / ${prog.total_files || 0} · 문장: ${sents}개 · 어절: ${eojeols}개`;
        }

        // 3. 실시간 로그 출력
        if (builderProgressLog && Array.isArray(prog.log_lines)) {
          const lines = prog.log_lines.slice(-50);
          builderProgressLog.innerHTML = lines.map(l => `<div>${escapeHtml(l)}</div>`).join('');
          builderProgressLog.scrollTop = builderProgressLog.scrollHeight;
        }

        // 4. 상태 완료/오류/중단 감지
        if (prog.status === 'COMPLETED') {
          stopBuildPolling();
          btnStartBuild.disabled = false;
          if (builderProgressPct) builderProgressPct.textContent = '100.0%';
          if (builderProgressBar) builderProgressBar.style.width = '100%';
          if (globalBuildTracker) globalBuildTracker.style.display = 'none';

          // 결과 요약 배너 표시
          const summary = prog.result_summary || {};
          const sents = (prog.sentences_imported || summary.sentences_imported || 0).toLocaleString();
          const eojeols = (prog.eojeols_imported || summary.eojeols_imported || 0).toLocaleString();
          const docsCount = (prog.newly_created_doc_ids && prog.newly_created_doc_ids.length) || summary.documents_created || 0;
          const rawOnlyCount = summary.raw_only_count || 0;
          const rawOnlyFiles = Array.isArray(summary.raw_only_files) ? summary.raw_only_files : [];

          if (builderSummaryBanner) {
            const rawOnlyNoticeHtml = rawOnlyCount > 0 ? `
              <div style="margin-top:10px; padding:10px 12px; border-radius:6px; background:#fff7ed; border:1px solid #fdba74; color:#9a3412; font-size:0.92em; line-height:1.5;">
                <strong>ℹ️ 평문 전용 DB 구축 안내 (${rawOnlyCount}개 파일):</strong><br/>
                지원되는 주석 말뭉치 형식(세종·학습자·국어원 JSON 등)과 달라 형태·의미·구문·벡터 층위 DB는 생성하지 않고 <strong>평문 말뭉치 DB만</strong> 구축했습니다.<br/>
                <span style="word-break:break-all;">대상 파일: <code>${escapeHtml(rawOnlyFiles.join(', '))}</code></span>
              </div>
            ` : '';

            builderSummaryBanner.innerHTML = `
              <strong>🎉 데이터베이스 생성이 성공적으로 완료되었습니다!</strong><br/>
              대상 DB: <code>${escapeHtml(prog.target_db_path || 'concordance.corpdb')}</code><br/>
              처리 완료 파일: <strong>${prog.total_files || 0}개</strong> · 등록된 말뭉치: <strong>${docsCount}개</strong> · 총 구축 문장: <strong>${sents}개</strong> (${eojeols}어절)
              ${rawOnlyNoticeHtml}
            `;
          }

          if (builderItemsTbody && summary.items) {
            builderItemsTbody.innerHTML = (summary.items || []).map(item => `
              <tr>
                <td><code><strong>${escapeHtml(item.doc_id)}</strong></code></td>
                <td>
                  <span class="layer-badge badge-old">${escapeHtml(item.type)}</span>
                  ${item.raw_only ? '<span class="layer-badge" style="margin-left:4px; background:#ffedd5; color:#9a3412; border:1px solid #fdba74;">평문 전용 · 주석 미생성</span>' : ''}
                </td>
                <td><code>${escapeHtml((item.files || []).join(', '))}</code></td>
                <td style="text-align:right; font-weight:700;">${(item.sentences || 0).toLocaleString()}문장</td>
              </tr>
            `).join('');
          }
          if (builderResultArea) builderResultArea.style.display = 'block';

          // [핵심] 구축된 DB를 즉시 반영하여 '문서지정' 탭과 검색기 목록 새로고침
          await loadOverview();
          const rawOnlyAlert = rawOnlyCount > 0
            ? `\n\n[안내] 지원되는 주석 형식과 다른 ${rawOnlyCount}개 파일(${rawOnlyFiles.join(', ')})은 형태·의미·구문·벡터 DB를 생성하지 않고 평문 말뭉치 DB만 구축했습니다.`
            : '';
          alert(`🎉 데이터베이스 생성이 완료되었습니다!\n\n총 ${prog.total_files || 0}개 파일 중 ${docsCount}개 말뭉치 (${sents}문장)가 DB에 성공적으로 등록되었습니다.\n지금 바로 검색하거나 '문서지정' 탭에서 확인하실 수 있습니다.${rawOnlyAlert}`);
        } else if (prog.status === 'ERROR') {
          stopBuildPolling();
          btnStartBuild.disabled = false;
          if (globalBuildTracker) globalBuildTracker.style.display = 'none';
          alert(`데이터베이스 구축 오류:\n${prog.error_message || '생성 도중 오류가 발생했습니다.'}`);
        } else if (prog.status === 'STOPPED' || prog.status === 'CANCELLED') {
          stopBuildPolling();
          btnStartBuild.disabled = false;
          if (globalBuildTracker) globalBuildTracker.style.display = 'none';
          await loadOverview();
        }
      } catch (err) {
        console.warn('Build progress polling error:', err);
      }
    }, 250);
  }

  // 2. Folder-based DB Builder 시작 리스너
  btnStartBuild.addEventListener('click', async () => {
    const folderPath = builderFolderPath.value.trim();
    const dbName = builderDbName.value.trim();
    if (!folderPath && (!selectedBuildFilePaths || selectedBuildFilePaths.length === 0)) {
      alert('말뭉치 폴더 또는 파일 경로를 입력해 주세요.');
      builderFolderPath.focus();
      return;
    }

    let filesToSend = null;
    if (selectedBuildFilePaths && selectedBuildFilePaths.length > 0) {
      if (builderFolderPath.value.includes(`선택한 ${selectedBuildFilePaths.length}개 파일`) || builderFolderPath.value === selectedBuildFilePaths[0]) {
        filesToSend = selectedBuildFilePaths;
      }
    }

    btnStartBuild.disabled = true;
    if (builderStatusBadge) {
      builderStatusBadge.style.display = 'inline-block';
      builderStatusBadge.innerHTML = '⏳ 말뭉치 스캔 및 데이터베이스 생성 프로세스 시작 중…';
    }

    try {
      const res = await fetch('/api/build-db', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          folder_path: folderPath,
          file_paths: filesToSend,
          db_name: dbName,
        }),
      });
      const data = await res.json();
      if (builderStatusBadge) builderStatusBadge.style.display = 'none';

      if (data.error) {
        btnStartBuild.disabled = false;
        alert(`데이터베이스 구축 오류:\n${data.error}`);
        return;
      }

      // 백그라운드 프로세스가 시작되었으므로 실시간 % 폴링 시작!
      startBuildPolling();
    } catch (err) {
      btnStartBuild.disabled = false;
      if (builderStatusBadge) builderStatusBadge.style.display = 'none';
      alert(`데이터베이스 생성 요청 실패: ${err.message}`);
    }
  });

  // 페이지 로드 시 혹시 이미 백엔드에서 빌드가 진행 중인지 상태 확인
  (async function checkExistingBuildOnLoad() {
    try {
      const res = await fetch('/api/build-db/status');
      if (res.ok) {
        const st = await res.json();
        if (st && st.is_running) {
          startBuildPolling();
        }
      }
    } catch (e) {}
  })();

  btnJumpToSearch.addEventListener('click', () => {
    document.querySelector('.nav-tab[data-tab="tab-search"]').click();
    searchInput.value = '';
    automata.reset();
    selectedDocIds = new Set(['ALL']);
    updateDocSelectorTriggerLabel();
    runSearch();
  });


  // =========================================================================
  // Single-Target & Multi-Method Search Configuration Modal Window Logic
  // =========================================================================
  function getSelectedSearchTargets() {
    if (chkTargetMorph && chkTargetMorph.checked) return ['morph'];
    if (chkTargetSense && chkTargetSense.checked) return ['sense'];
    if (chkTargetSyntax && chkTargetSyntax.checked) return ['syntax'];
    if (chkTargetVector && chkTargetVector.checked) return ['vector'];
    return ['surface'];
  }

  function getSelectedSearchMethods() {
    const methods = [];
    if (chkMethodJamo && chkMethodJamo.checked) methods.push('jamo');
    if (chkMethodRegex && chkMethodRegex.checked) methods.push('regex');
    // 자소 검색과 정규식 검색을 둘 다 선택하지 않으면 기본 텍스트 검색 적용
    return methods.length > 0 ? methods : ['text'];
  }

  function hasRegexSyntaxJs(q) {
    if (!q) return false;
    return /(?:\.\*|\.\+|\.\?|\.\{\d|\{\d+,\d*\}|\\[dwsbDWSB]|[\^\$]|[^\[\/]\+|[^\[\/]\*)/.test(q);
  }

  function updateSearchMethodSummaryLabel() {
    const targets = getSelectedSearchTargets();
    const methods = getSelectedSearchMethods();

    let targetStr = '평문';
    if (targets.includes('morph')) targetStr = '형태';
    else if (targets.includes('sense')) targetStr = '의미';
    else if (targets.includes('syntax')) targetStr = '구문';
    else if (targets.includes('vector')) targetStr = '벡터';

    let methodStr = '기본';
    const hasJamo = methods.includes('jamo');
    const hasRegex = methods.includes('regex');
    if (hasJamo && hasRegex) {
      methodStr = '자소+정규식';
    } else if (hasJamo) {
      methodStr = '자소';
    } else if (hasRegex) {
      methodStr = '정규식';
    }

    if (searchMethodSummaryLabel) {
      searchMethodSummaryLabel.textContent = `${targetStr} · ${methodStr}`;
    }
    if (methodModalSummaryText) {
      methodModalSummaryText.textContent = `${targetStr} · ${methodStr === '기본' ? '기본 텍스트' : methodStr}`;
    }
  }

  if (btnSearchMethodTrigger && modalSearchMethod) {
    // Open modal
    btnSearchMethodTrigger.addEventListener('click', () => {
      modalSearchMethod.style.display = 'flex';
      updateSearchMethodSummaryLabel();
    });

    // Close modal
    const closeMethodModal = () => {
      modalSearchMethod.style.display = 'none';
    };

    if (btnCloseMethodModal) btnCloseMethodModal.addEventListener('click', closeMethodModal);
    if (btnCancelMethodModal) btnCancelMethodModal.addEventListener('click', closeMethodModal);

    modalSearchMethod.addEventListener('click', (e) => {
      if (e.target === modalSearchMethod) {
        closeMethodModal();
      }
    });

    // Apply button
    if (btnApplyMethodModal) {
      btnApplyMethodModal.addEventListener('click', () => {
        closeMethodModal();
        updateSearchMethodSummaryLabel();
        runSearch();
      });
    }

    // Radio & checkbox change handlers
    [chkTargetSurface, chkTargetMorph, chkTargetSense, chkTargetSyntax, chkTargetVector,
     chkMethodJamo, chkMethodRegex].forEach((chk) => {
      if (chk) {
        chk.addEventListener('change', () => {
          updateSearchMethodSummaryLabel();
        });
      }
    });

    updateSearchMethodSummaryLabel();
  }

  function extractMatchedEojeolAnnotation(nodeStr, flatStr) {
    if (!nodeStr || !flatStr) return '';
    const nodeWords = nodeStr.trim().split(/\s+/).filter(Boolean);
    if (nodeWords.length === 0) return '';
    const eas = flatStr.split(' | ');
    const matchedEas = [];
    for (const ea of eas) {
      const openIdx = ea.indexOf('[');
      const surf = openIdx > 0 ? ea.slice(0, openIdx).trim() : ea.trim();
      if (nodeWords.some((nw) => surf === nw || surf.includes(nw) || nw.includes(surf))) {
        matchedEas.push(ea.trim());
      }
    }
    if (matchedEas.length > 0) {
      return matchedEas.slice(0, 3).join(' | ');
    }
    return '';
  }

  // KWIC 테이블 행 생성 헬퍼
  function createKwicRowHtml(h) {
    const badges = [];
    const hitLayers = h.annotation_layers || '';
    if (h.corpus_category && h.corpus_category.includes('옛한글')) {
      badges.push('<span class="layer-badge badge-old">옛한글</span>');
    }
    if (hitLayers.includes('평문') || (hitLayers.includes('원문') && !hitLayers.includes('형태') && !hitLayers.includes('의미') && !hitLayers.includes('구문')) || hitLayers.includes('원문(')) {
      badges.push('<span class="layer-badge badge-old">평문</span>');
    }
    if (h.has_morph || hitLayers.includes('형태')) {
      badges.push('<span class="layer-badge badge-morph">형태</span>');
    }
    if (h.has_sense || hitLayers.includes('의미')) {
      badges.push('<span class="layer-badge badge-sense">의미</span>');
    }
    if (h.has_syntax || hitLayers.includes('구문')) {
      badges.push('<span class="layer-badge badge-syntax">구문</span>');
    }
    if (hitLayers.includes('벡터')) {
      badges.push('<span class="layer-badge badge-syntax">벡터</span>');
    }
    const loc = h.page_ref
      ? `${h.doc_id} (${h.page_ref})`
      : `${h.doc_id} #${h.seq_no}`;

    const isMorphMatch = (h.match_layer && h.match_layer.includes('형태') && !h.match_layer.includes('의미')) || (chkTargetMorph && chkTargetMorph.checked);
    const isSenseMatch = (h.match_layer && h.match_layer.includes('의미')) || (chkTargetSense && chkTargetSense.checked);
    const isSyntaxMatch = (h.match_layer && h.match_layer.includes('구문')) || (chkTargetSyntax && chkTargetSyntax.checked);

    let nodeSubHtml = '';
    let rightLayerHtml = '';
    if (isMorphMatch && h.morph_flat) {
      const nodeMorph = extractMatchedEojeolAnnotation(h.node, h.morph_flat);
      if (nodeMorph) {
        nodeSubHtml = `<div class="kwic-node-analysis">${escapeHtml(nodeMorph)}</div>`;
      }
      rightLayerHtml = `<div class="kwic-layer-subline">🧩 <strong>형태분석:</strong> ${escapeHtml(h.morph_flat)}</div>`;
    } else if (isSenseMatch && h.sense_flat) {
      const nodeSense = extractMatchedEojeolAnnotation(h.node, h.sense_flat);
      if (nodeSense) {
        nodeSubHtml = `<div class="kwic-node-analysis sense">${escapeHtml(nodeSense)}</div>`;
      }
      rightLayerHtml = `<div class="kwic-layer-subline sense">🔖 <strong>형태의미:</strong> ${escapeHtml(h.sense_flat)}</div>`;
    } else if (isSyntaxMatch && h.syntax_tree) {
      rightLayerHtml = `<div class="kwic-layer-subline syntax">🌲 <strong>구문분석:</strong> ${escapeHtml(h.syntax_tree)}</div>`;
    }

    return `
      <tr class="kwic-row" data-seg-id="${h.segment_id}" data-text="${escapeHtml(h.text_unicode)}" title="클릭: 상세 뷰어 / 더블클릭: 뷰어 펼치기 / 마우스 우클릭: 원본 파일 점프">
        <td>
          <div style="font-weight:700; font-size:0.76rem; color:#1e293b;">${escapeHtml(loc)}</div>
          <div style="font-size:0.7rem; color:#64748b;">[${escapeHtml(h.match_layer)}]</div>
        </td>
        <td>${badges.join('')}</td>
        <td class="kwic-left">${escapeHtml(h.left_context)}</td>
        <td class="kwic-node-cell"><span class="kwic-node">${escapeHtml(h.node || '·')}</span>${nodeSubHtml}</td>
        <td class="kwic-right"><div>${escapeHtml(h.right_context)}</div>${rightLayerHtml}</td>
      </tr>
    `;
  }

  function bindKwicRowItemEvents(r, tab) {
    r.addEventListener('click', () => {
      kwicTbody.querySelectorAll('.kwic-row').forEach((x) => x.classList.remove('selected'));
      r.classList.add('selected');
      tab.selectedSegId = r.dataset.segId;
      loadSegmentDetail(r.dataset.segId);
    });
    r.addEventListener('dblclick', () => {
      setInspectorExpanded(true);
    });
    r.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      kwicTbody.querySelectorAll('.kwic-row').forEach((x) => x.classList.remove('selected'));
      r.classList.add('selected');
      rightClickedSegId = r.dataset.segId;
      showContextMenu(e.clientX, e.clientY);
    });
  }

  // [요청 반영] 실시간 스트리밍 검색 (검색된 문장이 검색되자마자 즉시 화면에 행 추가)
  async function runSearch() {
    const q = searchInput.value;
    const selectedMethods = getSelectedSearchMethods();
    // [자소 검색 자동 활성화] 검색어에 자소 검색 문법('[', ']', '/')이 포함되어 있으면 자소 검색 모드 자동 추가
    if (q.includes('[') && q.includes(']') && q.includes('/') && !selectedMethods.includes('jamo')) {
      selectedMethods.push('jamo');
    }
    // [정규식 자동 활성화] 검색어에 정규표현식 패턴(.{1,10}, .*, .+ 등)이 포함되어 있으면 정규식 모드 자동 추가
    if (hasRegexSyntaxJs(q) && !selectedMethods.includes('regex')) {
      selectedMethods.push('regex');
    }
    const targets = getSelectedSearchTargets().join(',');
    const methods = selectedMethods.join(',');
    const searchUnit = selectSearchUnit ? selectSearchUnit.value : 'eojeol';
    const mode = selectMode ? selectMode.value : targets;
    const layer = selectLayer ? selectLayer.value : 'ALL';
    const isAllDocs = selectedDocIds.has('ALL') || selectedDocIds.size === globalDocuments.length || selectedDocIds.size === 0;
    const docIdsParam = isAllDocs ? 'ALL' : Array.from(selectedDocIds).join(',');
    const docId = isAllDocs ? 'ALL' : (selectedDocIds.size === 1 ? Array.from(selectedDocIds)[0] : 'MULTI');
    const contextWords = selectContextWords.value;
    const limit = selectLimit.value;

    if (!q.trim() && isAllDocs && targets === 'surface') {
      kwicResultMeta.innerHTML = '<span style="color:#64748b;">검색어를 입력하거나 특정 문서(또는 형태소/품사 등 분석 층위)를 선택해 주세요.</span>';
      return;
    }

    searchExplicitlyCancelled = false;
    if (currentSearchAbort) {
      try {
        currentSearchAbort.abort();
      } catch (_) {}
      currentSearchAbort = null;
    }

    const mySearchSeq = ++currentSearchSeq;
    const myAbortController = new AbortController();
    currentSearchAbort = myAbortController;
    isSearchInProgress = true;

    btnCancelSearch.style.display = 'inline-flex';
    if (btnSearch) {
      btnSearch.disabled = true;
      btnSearch.innerHTML = '⏳ 검색 중...';
    }

    const ctxLabel = contextWords === '999' ? '문장 전체' : `좌우 ${contextWords}어절`;
    const unitLabel = SEARCH_UNIT_BASE_LABELS[searchUnit] || '어절 단위 검색';
    const targetLayerLabel = targets === 'morph' ? '형태소 분석 말뭉치' : targets === 'sense' ? '형태의미 말뭉치' : targets === 'syntax' ? '구문분석 말뭉치' : '전체 문서';
    let docLabel = targetLayerLabel;
    if (selectedDocIds && !selectedDocIds.has('ALL') && selectedDocIds.size > 0) {
      docLabel = selectedDocIds.size === 1 ? Array.from(selectedDocIds)[0] : `${selectedDocIds.size}개 문서`;
    }

    kwicResultTitle.textContent = q
      ? `용례 검색 결과: "${q}" [${docLabel} · ${unitLabel}] (${ctxLabel})`
      : `말뭉치 열람 (${docLabel} · ${ctxLabel})`;

    kwicResultMeta.innerHTML = '<strong style="color:#2563eb;">⚡ 실시간 검색 시작… 문장이 발견되는 즉시 아래에 바로 추가됩니다.</strong>';
    kwicTbody.innerHTML = ''; // 이전 결과 비우고 즉시 대기 화면 구축

    // 검색 탭 생성 또는 재사용
    const tabTitle = q || (targets !== 'surface' ? targetLayerLabel : getDocSelectionSummaryLabel());
    let targetTab = null;
    if (searchTabs.length > 0 && activeTabId) {
      const curTab = searchTabs.find((t) => t.id === activeTabId);
      if (curTab && !curTab.query && (!curTab.data || !curTab.data.hits || curTab.data.hits.length === 0)) {
        targetTab = curTab;
      }
    }

    if (!targetTab) {
      if (searchTabs.length >= 10) {
        searchTabs.shift();
      }
      targetTab = {
        id: 'tab_' + Date.now() + '_' + Math.random().toString(36).slice(2, 6),
        query: q,
        title: tabTitle,
        mode,
        targets,
        searchUnit,
        layer,
        docId,
        docIds: new Set(selectedDocIds),
        contextWords,
        limit,
        data: {
          query: q,
          total_matches: 0,
          returned_count: 0,
          elapsed_ms: 0,
          cancelled: false,
          hits: [],
        },
        subsearchQuery: '',
        selectedSegId: null,
      };
      searchTabs.push(targetTab);
      activeTabId = targetTab.id;
    } else {
      targetTab.query = q;
      targetTab.title = tabTitle;
      targetTab.mode = mode;
      targetTab.targets = targets;
      targetTab.searchUnit = searchUnit;
      targetTab.layer = layer;
      targetTab.docId = docId;
      targetTab.docIds = new Set(selectedDocIds);
      targetTab.contextWords = contextWords;
      targetTab.limit = limit;
      targetTab.data = {
        query: q,
        total_matches: 0,
        returned_count: 0,
        elapsed_ms: 0,
        cancelled: false,
        hits: [],
      };
      targetTab.subsearchQuery = '';
      targetTab.selectedSegId = null;
    }

    renderTabsBar();

    const qs = new URLSearchParams({
      q,
      mode,
      targets,
      methods,
      search_unit: searchUnit,
      layer_filter: layer,
      doc_id: docId,
      doc_ids: docIdsParam,
      context_words: contextWords,
      limit,
    });

    try {
      // [핵심] 1차: 고속 실시간 점진적 스트리밍 (/api/search/stream)
      // 404 또는 구버전 서버 접속 시 자동으로 표준 검색(/api/search)으로 완벽 Fallback!
      let streamSuccess = false;
      try {
        const streamRes = await fetch(`/api/search/stream?${qs.toString()}`, {
          signal: myAbortController.signal,
        });

        if (streamRes.ok) {
          streamSuccess = true;
          const reader = streamRes.body.getReader();
          const decoder = new TextDecoder('utf-8');
          let buffer = '';
          let isFirstHit = true;

          while (true) {
            const { value, done } = await reader.read();
            if (done) break;

            buffer += decoder.decode(value, { stream: true });
            const lines = buffer.split('\n');
            buffer = lines.pop(); // 남아있는 불완전한 줄은 버퍼에 보관

            for (const line of lines) {
              if (!line.trim()) continue;
              let msg = null;
              try {
                msg = JSON.parse(line);
              } catch (_) {
                continue;
              }

              if (msg.type === 'start') {
                kwicResultMeta.innerHTML = `<strong style="color:#2563eb;">⚡ 실시간 검색 중… 검색된 문장이 발견되는 즉시 아래에 표시됩니다. (0건)</strong>`;
              } else if (msg.type === 'hit' && msg.hit) {
                // 검색된 문장이 검색되자마자 바로 화면에 추가!
                const hit = msg.hit;
                targetTab.data.hits.push(hit);
                targetTab.data.returned_count = targetTab.data.hits.length;
                targetTab.data.total_matches = msg.matched_count || targetTab.data.hits.length;

                const tempTbody = document.createElement('tbody');
                tempTbody.innerHTML = createKwicRowHtml(hit);
                const newRow = tempTbody.firstElementChild;
                if (newRow) {
                  bindKwicRowItemEvents(newRow, targetTab);
                  kwicTbody.appendChild(newRow);

                  if (isFirstHit) {
                    isFirstHit = false;
                    newRow.classList.add('selected');
                    targetTab.selectedSegId = hit.segment_id;
                    loadSegmentDetail(hit.segment_id);
                  }
                }

                kwicResultMeta.innerHTML = `<strong style="color:#2563eb;">⚡ 실시간 검색 중… 현재 ${targetTab.data.hits.length.toLocaleString()}건 발견되어 바로 표시 중</strong>`;
              } else if (msg.type === 'done') {
                targetTab.data.total_matches = msg.total_matches || targetTab.data.hits.length;
                targetTab.data.returned_count = targetTab.data.hits.length;
                targetTab.data.elapsed_ms = Math.round(msg.elapsed_ms || 0);
                targetTab.data.cancelled = msg.cancelled || false;
              } else if (msg.type === 'error') {
                kwicResultMeta.innerHTML = `<strong style="color:#dc2626;">오류: ${escapeHtml(msg.error)}</strong>`;
              }
            }
          }
        }
      } catch (streamErr) {
        if (streamErr.name === 'AbortError') throw streamErr;
        console.warn('Streaming search failed or 404, falling back to standard /api/search:', streamErr);
        streamSuccess = false;
      }

      // [Fallback] 서버가 아직 재시작되지 않아 404가 발생했거나 스트리밍 실패 시 표준 API로 자동 조회
      if (!streamSuccess) {
        const stdRes = await fetch(`/api/search?${qs.toString()}`, {
          signal: myAbortController.signal,
        });
        if (!stdRes.ok) {
          throw new Error(`서버 응답 오류 (HTTP ${stdRes.status})`);
        }
        const data = await stdRes.json();
        targetTab.data = data;

        kwicTbody.innerHTML = '';
        if (data.hits && data.hits.length > 0) {
          let isFirst = true;
          for (let i = 0; i < data.hits.length; i++) {
            const hit = data.hits[i];
            const tempTbody = document.createElement('tbody');
            tempTbody.innerHTML = createKwicRowHtml(hit);
            const newRow = tempTbody.firstElementChild;
            if (newRow) {
              bindKwicRowItemEvents(newRow, targetTab);
              kwicTbody.appendChild(newRow);
              if (isFirst) {
                isFirst = false;
                newRow.classList.add('selected');
                targetTab.selectedSegId = hit.segment_id;
                loadSegmentDetail(hit.segment_id);
              }
            }
          }
        }
      }

      if (mySearchSeq !== currentSearchSeq) return;

      isSearchInProgress = false;
      btnCancelSearch.style.display = 'none';
      if (btnSearch) {
        btnSearch.disabled = false;
        btnSearch.innerHTML = '🔍 검색';
      }
      currentSearchAbort = null;

      const limitNote = targetTab.limit === '0' ? '무제한' : `최대 ${Number(targetTab.limit).toLocaleString()}건`;
      const cancelNote = targetTab.data.cancelled ? ' · <strong style="color:#dc2626;">[중단됨: 부분 결과]</strong>' : '';

      if (targetTab.data.hits.length === 0) {
        kwicTbody.innerHTML = `
          <tr>
            <td colspan="5" style="text-align:center; padding:36px; color:#64748b;">
              조건에 일치하는 문장이 없습니다.
            </td>
          </tr>
        `;
        kwicResultMeta.innerHTML = `검색 결과 없음 · 소요시간 <strong>${targetTab.data.elapsed_ms || 0}ms</strong>${cancelNote}`;
      } else {
        kwicResultMeta.innerHTML = `총 <strong>${(targetTab.data.total_matches || targetTab.data.hits.length).toLocaleString()}건</strong> 매칭 (표시 ${targetTab.data.returned_count.toLocaleString()}건 / ${limitNote}) · 소요시간 <strong>${targetTab.data.elapsed_ms || 0}ms</strong>${cancelNote}`;
      }

            renderTabsBar();
    } catch (err) {
      if (mySearchSeq !== currentSearchSeq) return;

      isSearchInProgress = false;
      btnCancelSearch.style.display = 'none';
      if (btnSearch) {
        btnSearch.disabled = false;
        btnSearch.innerHTML = '🔍 검색';
      }

      if (err.name === 'AbortError') {
        if (searchExplicitlyCancelled) {
          kwicResultMeta.innerHTML = `<strong style="color:#dc2626;">⏹ 검색이 사용자에 의해 중단되었습니다. (현재까지 표시 ${targetTab ? targetTab.data.hits.length : 0}건)</strong>`;
        }
      } else {
        kwicResultMeta.textContent = `검색 오류: ${err.message}`;
      }
    }
  }

  async function loadSegmentDetail(segId) {
    try {
      const res = await fetch(`/api/segment?id=${encodeURIComponent(segId)}`);
      const d = await res.json();
      if (d.error) return;
      currentSegmentDetail = d;
      editModeActive = false;
      btnToggleEdit.style.display = 'inline-flex';
      btnToggleEdit.textContent = '✏️ DB 직접 수정';
      updateInspectorHeaderSummary();
      renderInspector();
    } catch (err) {
      console.error('Detail error:', err);
    }
  }

  btnToggleEdit.addEventListener('click', (e) => {
    e.stopPropagation();
    if (!currentSegmentDetail) return;
    if (!inspectorExpanded) {
      setInspectorExpanded(true);
    }
    editModeActive = !editModeActive;
    btnToggleEdit.textContent = editModeActive ? '✕ 수정 취소' : '✏️ DB 직접 수정';
    renderInspector();
  });

  function renderInspector() {
    const d = currentSegmentDetail;
    if (!d) return;

    if (editModeActive) {
      inspectorBody.innerHTML = `
        <div class="detail-section">
          <div class="detail-label">✏️ 데이터베이스 레코드 직접 수정 (Segment #${d.segment_id} · ${escapeHtml(d.doc_id)})</div>
          <label style="font-size:0.78rem; font-weight:700;">원문 (유니코드 첫가끝 / 현대한글):</label>
          <input type="text" id="edit-text-unicode" class="edit-input" value="${escapeHtml(d.text_display)}" />

          <label style="font-size:0.78rem; font-weight:700;">형태소 분석 (Morphological Layer):</label>
          <textarea id="edit-morph" class="edit-input" rows="2">${escapeHtml(d.morph_flat)}</textarea>

          <label style="font-size:0.78rem; font-weight:700;">형태의미 분석 (Morpho-Semantic Layer):</label>
          <textarea id="edit-sense" class="edit-input" rows="2">${escapeHtml(d.sense_flat)}</textarea>

          <label style="font-size:0.78rem; font-weight:700;">구문 분석 트리 (Syntax Tree S-Expression):</label>
          <textarea id="edit-syntax" class="edit-input" rows="4">${escapeHtml(d.syntax_tree)}</textarea>

          <div style="display:flex; gap:8px; margin-top:6px;">
            <button type="button" class="btn btn-primary" id="btn-save-segment">💾 DB에 변경사항 즉시 저장</button>
          </div>
        </div>
      `;

      document.getElementById('btn-save-segment').addEventListener('click', async () => {
        const payload = {
          segment_id: d.segment_id,
          text_unicode: document.getElementById('edit-text-unicode').value,
          morph_flat: document.getElementById('edit-morph').value,
          sense_flat: document.getElementById('edit-sense').value,
          syntax_tree: document.getElementById('edit-syntax').value,
        };
        const r = await fetch('/api/segment/edit', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload),
        });
        const resData = await r.json();
        if (resData.ok) {
          await loadSegmentDetail(d.segment_id);
        }
      });
      return;
    }

    let eojeolHtml = '';
    if (Array.isArray(d.eojeols) && d.eojeols.length > 0) {
      eojeolHtml = `
        <div class="detail-section">
          <div class="detail-label">
            <span>🧩 어절별 형태소 · 형태의미(어깨번호) 연동 분석 (${d.eojeols.length}어절)</span>
          </div>
          <div class="eojeol-grid">
            ${d.eojeols
              .map(
                (ej) => `
              <div class="eojeol-card">
                <div class="eojeol-surface">${escapeHtml(ej.surface || ej.word || '')} <small style="color:#64748b; font-weight:400;">${escapeHtml(ej.ej_id || ej.id || '')}</small></div>
                ${ej.morph ? `<div class="eojeol-morph"><strong>[형태]</strong> ${escapeHtml(ej.morph)}</div>` : ''}
                ${ej.sense ? `<div class="eojeol-sense"><strong>[의미]</strong> ${escapeHtml(ej.sense)}</div>` : ''}
              </div>
            `
              )
              .join('')}
          </div>
        </div>
      `;
    } else if (d.morph_flat || d.sense_flat) {
      eojeolHtml = `
        <div class="detail-section">
          <div class="detail-label">🧩 형태소 / 형태의미 분석 결과</div>
          ${d.morph_flat ? `<div style="margin-bottom:4px;" class="corpus-text"><strong>[형태 분석]</strong> ${escapeHtml(d.morph_flat)}</div>` : ''}
          ${d.sense_flat ? `<div style="color:#92400e;" class="corpus-text"><strong>[의미 분석]</strong> ${escapeHtml(d.sense_flat)}</div>` : ''}
        </div>
      `;
    }

    const syntaxHtml = d.syntax_tree
      ? `
      <div class="detail-section">
        <div class="detail-label">🌲 연동된 구문 분석 트리 (Sejong S-Expression Syntax Tree)</div>
        <pre class="syntax-pre">${escapeHtml(d.syntax_tree)}</pre>
      </div>
    `
      : '';

    const rawDiffHtml =
      d.text_raw !== d.text_display
        ? `
      <div style="margin-top:5px; font-size:0.8rem; color:#475569;">
        <strong>원본 표기(한양 PUA / 원시 태그):</strong>
        <span class="corpus-text" style="background:#f1f5f9; padding:2px 6px; border-radius:4px;">${escapeHtml(d.text_raw)}</span>
      </div>
    `
        : '';

    const neighborsHtml =
      Array.isArray(d.neighbors) && d.neighbors.length > 1
        ? `
      <div class="detail-section">
        <div class="detail-label">📖 동일 문헌 앞뒤 문맥 (±3문장 클릭 이동)</div>
        <div style="font-size:0.84rem;">
          ${d.neighbors
            .map(
              (n) => `
            <div
              class="neighbor-item"
              data-seg="${n.segment_id}"
              style="padding:3px 8px; border-radius:4px; cursor:pointer; margin-bottom:2px; ${
                n.segment_id === d.segment_id
                  ? 'background:#dbeafe; font-weight:700; color:#1e3a8a;'
                  : 'color:#334155;'
              }"
            >
              <span style="font-size:0.74rem; color:#64748b;">[#${n.seq_no}${n.page_ref ? ' · ' + escapeHtml(n.page_ref) : ''}]</span>
              <span class="corpus-text">${escapeHtml(n.text_unicode)}</span>
            </div>
          `
            )
            .join('')}
        </div>
      </div>
    `
        : '';

    const editsHtml =
      Array.isArray(d.edits) && d.edits.length > 0
        ? `
      <div class="detail-section">
        <div class="detail-label">🕓 DB 수정 이력 (Delta Audit Log)</div>
        ${d.edits
          .map(
            (ed) => `
          <div style="font-size:0.76rem; color:#475569; margin-bottom:3px;">
            [${escapeHtml(ed.edited_at)}] <strong>${escapeHtml(ed.field_name)}</strong> 수정됨
          </div>
        `
          )
          .join('')}
      </div>
    `
        : '';

    inspectorBody.innerHTML = `
      <div class="detail-section">
        <div class="detail-label">
          <span>📄 출전 문헌 및 연동 원본 파일</span>
          <span>Segment #${d.segment_id} (문장 순번 #${d.seq_no})</span>
        </div>
        <div style="font-size:0.82rem; background:#f8fafc; padding:6px 10px; border-radius:6px; border:1px solid #e2e8f0;">
          <div><strong>문서명:</strong> ${escapeHtml(d.doc_title)} (<code>${escapeHtml(d.doc_id)}</code>) · <strong>연동 원본:</strong> <code>${escapeHtml(d.source_files_list)}</code></div>
          <div>
            <strong>주석 층위:</strong> ${escapeHtml(d.annotation_layers)}
            ${d.page_ref ? ` · <strong>장차(Page):</strong> <code>${escapeHtml(d.page_ref)}</code>` : ''}
            ${d.sent_id ? ` · <strong>문장 ID:</strong> <code>${escapeHtml(d.sent_id)}</code>` : ''}
            ${d.lang ? ` · <strong>언어:</strong> <code>${escapeHtml(d.lang)}</code>` : ''}
          </div>
        </div>
      </div>

      <div class="detail-section">
        <div class="detail-label">📜 원문 문장</div>
        <div class="sentence-box">${escapeHtml(d.text_display)}</div>
        ${rawDiffHtml}
      </div>

      ${eojeolHtml}
      ${syntaxHtml}
      ${neighborsHtml}
      ${editsHtml}
    `;

    inspectorBody.querySelectorAll('.neighbor-item').forEach((el) => {
      el.addEventListener('click', () => {
        loadSegmentDetail(el.dataset.seg);
      });
    });
  }

  const btnQuickGotoBuilder = document.getElementById('btn-quick-goto-builder');
  if (btnQuickGotoBuilder) {
    btnQuickGotoBuilder.addEventListener('click', () => {
      document.querySelector('.nav-tab[data-tab="tab-build-db"]').click();
    });
  }
  let selectedBuildFilePaths = [];

  const btnBrowseFolder = document.getElementById('btn-browse-folder');
  if (btnBrowseFolder) {
    btnBrowseFolder.addEventListener('click', async () => {
      try {
        btnBrowseFolder.disabled = true;
        btnBrowseFolder.textContent = '⏳ 창 여는 중...';
        const res = await fetch('/api/system/pick-folder', { method: 'POST' });
        const data = await res.json();
        if (data.ok) {
          if (data.file_paths && data.file_paths.length > 0) {
            selectedBuildFilePaths = data.file_paths;
            if (builderFolderPath) {
              if (data.file_paths.length === 1) {
                builderFolderPath.value = data.file_paths[0];
              } else {
                builderFolderPath.value = `[선택한 ${data.file_paths.length}개 파일] ${data.file_paths[0]} 외`;
              }
              builderFolderPath.focus();
            }
          } else if (data.folder_path && builderFolderPath) {
            selectedBuildFilePaths = [];
            builderFolderPath.value = data.folder_path;
            builderFolderPath.focus();
          }
        }
      } catch (e) {
        console.warn('Folder/file picker error:', e);
      } finally {
        btnBrowseFolder.disabled = false;
        btnBrowseFolder.textContent = '📂 폴더/파일 찾아보기...';
      }
    });
  }

  // =========================================================================
  // 말뭉치 삭제 및 원문 코퍼스 재구축(Rebuild) 연동 기능
  // =========================================================================
  let lastRebuildSourcePath = '';

  async function deleteDocuments(docIds, confirmMessage) {
    if (!docIds || docIds.length === 0) return;
    if (!confirm(confirmMessage)) return;

    try {
      const qs = new URLSearchParams({ doc_ids: docIds.join(',') });
      const res = await fetch(`/api/document/delete?${qs.toString()}`, { method: 'POST' });
      const data = await res.json();

      if (data.error) {
        alert('삭제 실패: ' + data.error);
        return;
      }

      // 삭제된 문서가 현재 선택되어 있던 경우 선택 해제
      docIds.forEach(id => {
        selectedDocIds.delete(id);
        tempSelectedDocIds.delete(id);
      });
      if (selectedDocIds.size === 0) {
        selectedDocIds = new Set(['ALL']);
      }
      updateDocSelectorTriggerLabel();

      // 재구축 지원: 삭제된 문서들의 원본 파일 경로 기억
      if (data.deleted_items && data.deleted_items.length > 0) {
        const first = data.deleted_items[0];
        let srcPath = first.source_files || '';
        try {
          const arr = JSON.parse(srcPath);
          if (Array.isArray(arr) && arr.length > 0) {
            srcPath = arr[0];
          }
        } catch (_) {}
        if (srcPath && !srcPath.includes(':') && !srcPath.startsWith('/')) {
          srcPath = `sample/${srcPath}`;
        }
        lastRebuildSourcePath = srcPath;
        const btnFillRebuild = document.getElementById('btn-fill-rebuild-target');
        if (btnFillRebuild && lastRebuildSourcePath) {
          btnFillRebuild.style.display = 'inline-flex';
          btnFillRebuild.textContent = `🔄 최근 삭제한 [${first.title.slice(0, 15)}...] 원문 경로 채우기`;
        }
      }

      alert(`${data.message}\n\n💡 해당 원본 코퍼스는 [📁 말뭉치 폴더 지정 DB 생성] 탭에서 언제든지 다시 데이터베이스로 구축할 수 있습니다.`);
      await loadOverview();
    } catch (err) {
      alert('말뭉치 삭제 통신 오류: ' + err.message);
    }
  }

  function triggerRebuildForDoc(doc) {
    if (!doc) return;
    const tabBuildBtn = document.querySelector('.nav-tab[data-tab="tab-build-db"]');
    if (tabBuildBtn) tabBuildBtn.click();

    const inputFolder = document.getElementById('builder-folder-path');
    if (inputFolder) {
      let suggested = doc.source_files_list || '';
      try {
        const arr = JSON.parse(suggested);
        if (Array.isArray(arr) && arr.length > 0) {
          suggested = arr.length > 1 ? 'sample' : `sample/${arr[0]}`;
        }
      } catch (_) {}
      // sample 폴더 관련이면 상대경로 형태 채우기
      if (suggested && !suggested.includes(':') && !suggested.startsWith('/') && !suggested.startsWith('sample')) {
        suggested = `sample/${suggested}`;
      }
      if (!suggested) suggested = 'sample';
      inputFolder.value = suggested;
      inputFolder.focus();
    }
    alert(`[${doc.title}] 말뭉치의 원본 경로가 설정되었습니다.\n\n아래의 [🚀 데이터베이스 생성 시작] 버튼을 누르면 해당 원문 코퍼스가 데이터베이스로 다시 구축됩니다.`);
  }

  // [요청 1 반영] 문서 목록 체크박스 Shift 키 범위 다중 선택 및 일괄 삭제 연동
  let lastCheckedDocRowIndex = null;

  function updateSelectedDocsDeleteBtnCount() {
    const checkedBoxes = document.querySelectorAll('.chk-doc-row:checked');
    const btnDeleteSelected = document.getElementById('btn-delete-selected-docs');
    const btnSaveBundle = document.getElementById('btn-save-docs-to-user-bundle');
    const btnSearchSelected = document.getElementById('btn-search-selected-docs');
    const chkAll = document.getElementById('chk-doc-table-all');
    const allBoxes = document.querySelectorAll('.chk-doc-row');
    if (btnDeleteSelected) {
      if (checkedBoxes.length > 0) {
        btnDeleteSelected.textContent = `🗑️ 선택한 ${checkedBoxes.length}개 말뭉치 일괄 삭제`;
        btnDeleteSelected.style.fontWeight = '700';
      } else {
        btnDeleteSelected.textContent = '🗑️ 선택한 말뭉치 일괄 삭제';
        btnDeleteSelected.style.fontWeight = 'normal';
      }
    }
    if (btnSaveBundle) {
      btnSaveBundle.textContent = checkedBoxes.length > 0
        ? `📚 선택한 ${checkedBoxes.length}개 → 사용자말뭉치 저장`
        : '📚 선택한 말뭉치 → 사용자말뭉치 저장';
    }
    if (btnSearchSelected) {
      btnSearchSelected.textContent = checkedBoxes.length > 0
        ? `🔍 선택한 ${checkedBoxes.length}개 말뭉치에서 검색`
        : '🔍 선택한 말뭉치에서 검색';
    }
    if (chkAll && allBoxes.length > 0) {
      chkAll.checked = checkedBoxes.length === allBoxes.length;
    }
  }

  function bindDocRowCheckboxEvents() {
    const checkboxes = Array.from(document.querySelectorAll('.chk-doc-row'));
    checkboxes.forEach((chk, idx) => {
      chk.addEventListener('click', (e) => {
        // Shift 키를 누른 채 클릭한 경우 직전 선택 위치부터 현재 위치까지 범위 일괄 선택/해제
        if (e.shiftKey && lastCheckedDocRowIndex !== null && lastCheckedDocRowIndex !== idx) {
          const start = Math.min(lastCheckedDocRowIndex, idx);
          const end = Math.max(lastCheckedDocRowIndex, idx);
          const targetState = chk.checked;
          for (let i = start; i <= end; i++) {
            if (checkboxes[i]) {
              checkboxes[i].checked = targetState;
            }
          }
        }
        lastCheckedDocRowIndex = idx;
        updateSelectedDocsDeleteBtnCount();
      });
    });
  }

  // 상단 테이블 전체 선택 체크박스
  const chkDocTableAll = document.getElementById('chk-doc-table-all');
  if (chkDocTableAll) {
    chkDocTableAll.addEventListener('change', (e) => {
      const isChecked = e.target.checked;
      document.querySelectorAll('.chk-doc-row').forEach(c => c.checked = isChecked);
      updateSelectedDocsDeleteBtnCount();
    });
  }

  // 상단 선택한 말뭉치 일괄 삭제 버튼
  const btnDeleteSelectedDocs = document.getElementById('btn-delete-selected-docs');
  if (btnDeleteSelectedDocs) {
    btnDeleteSelectedDocs.addEventListener('click', () => {
      const checkedBoxes = Array.from(document.querySelectorAll('.chk-doc-row:checked'));
      if (checkedBoxes.length === 0) {
        alert('삭제할 말뭉치를 목록에서 먼저 체크해 주세요.\n\n💡 팁: 첫 번째 문서를 체크한 뒤 Shift를 누른 채 다른 문서를 클릭하면 그 사이의 문서들이 모두 한 번에 선택됩니다.');
        return;
      }
      const docIds = checkedBoxes.map(c => c.dataset.docId);
      deleteDocuments(docIds, `선택한 ${docIds.length}개 말뭉치를 데이터베이스에서 일괄 삭제하시겠습니까?\n\n※ 선택한 말뭉치들의 문장 데이터가 DB에서 영구 제거됩니다.\n(컴퓨터의 원본 텍스트 파일은 안전하게 보존되므로 언제든 다시 구축할 수 있습니다)`);
    });
  }

  // 재구축 경로 채우기 버튼 이벤트
  const btnFillRebuildTarget = document.getElementById('btn-fill-rebuild-target');
  if (btnFillRebuildTarget) {
    btnFillRebuildTarget.addEventListener('click', () => {
      const inputFolder = document.getElementById('builder-folder-path');
      if (inputFolder && lastRebuildSourcePath) {
        inputFolder.value = lastRebuildSourcePath;
        alert(`원문 코퍼스 경로 '${lastRebuildSourcePath}' 가 채워졌습니다.\n[🚀 데이터베이스 생성 시작]을 누르면 다시 DB로 구축됩니다.`);
      }
    });
  }

  // 메타데이터 상세 보기 모달 함수
  function openDocMetaModal(doc) {
    const modal = document.getElementById('modal-doc-meta');
    const titleEl = document.getElementById('modal-doc-meta-title');
    const bodyEl = document.getElementById('modal-doc-meta-body');
    if (!modal || !bodyEl) return;
    if (titleEl) {
      titleEl.textContent = `📋 메타데이터 전체 정보 — ${doc.title || doc.doc_id}`;
    }
    const renderChips = (raw) => {
      if (!raw || !raw.trim()) {
        return '<span style="color:#94a3b8; font-size:0.82rem;">수록된 정보가 없습니다.</span>';
      }
      return raw
        .split(' | ')
        .map((part) => `<span class="meta-tag-chip">${escapeHtml(part)}</span>`)
        .join('');
    };
    bodyEl.innerHTML = `
      <div class="meta-detail-section">
        <h4>👤 화자 정보 (연령 · 국적 · 성별 · 등급 등)</h4>
        <div class="meta-tag-list">${renderChips(doc.speaker_info)}</div>
      </div>
      <div class="meta-detail-section">
        <h4>🗂️ 기타 정보 (매체 · 장르 · 주제 · 환경 · 출처 등)</h4>
        <div class="meta-tag-list">${renderChips(doc.other_meta)}</div>
      </div>
      <div style="font-size:0.78rem; color:#64748b;">
        <strong>DB 문서 ID:</strong> <code>${escapeHtml(doc.doc_id)}</code> ·
        <strong>원본 파일:</strong> <code>${escapeHtml(doc.source_files_list || '')}</code>
      </div>
    `;
    modal.style.display = 'flex';
  }

  const modalDocMeta = document.getElementById('modal-doc-meta');
  const btnCloseDocMeta = document.getElementById('btn-close-doc-meta');
  const btnOkDocMeta = document.getElementById('btn-ok-doc-meta');
  [btnCloseDocMeta, btnOkDocMeta].forEach((b) => {
    if (b && modalDocMeta) {
      b.addEventListener('click', () => { modalDocMeta.style.display = 'none'; });
    }
  });
  if (modalDocMeta) {
    modalDocMeta.addEventListener('click', (e) => {
      if (e.target === modalDocMeta) modalDocMeta.style.display = 'none';
    });
  }

  // 구축 데이터베이스 문서 목록: 선택한 말뭉치 → 사용자말뭉치 저장
  const btnSaveDocsToUserBundle = document.getElementById('btn-save-docs-to-user-bundle');
  if (btnSaveDocsToUserBundle) {
    btnSaveDocsToUserBundle.addEventListener('click', () => {
      const checkedBoxes = Array.from(document.querySelectorAll('.chk-doc-row:checked'));
      if (checkedBoxes.length === 0) {
        alert('사용자말뭉치로 묶을 말뭉치를 목록에서 1개 이상 체크해 주세요.\n\n💡 팁: 첫 번째 문서를 체크한 뒤 Shift를 누른 채 다른 문서를 클릭하면 그 사이의 문서들이 모두 한 번에 선택됩니다.');
        return;
      }
      const docIds = checkedBoxes.map((c) => c.dataset.docId);
      const name = prompt(`선택한 ${docIds.length}개 말뭉치를 묶어 저장할 [사용자말뭉치] 이름을 입력하세요:`, '새 사용자말뭉치');
      if (!name || !name.trim()) return;
      const trimmed = name.trim();
      saveUserCorpusBundle(trimmed, docIds);
      activeUserBundleName = trimmed;
      selectedDocIds = new Set(docIds);
      updateDocSelectorTriggerLabel();
      refreshUserCorpusSelects();
      if (selectUserCorpus) selectUserCorpus.value = trimmed;
      alert(`사용자말뭉치 '${trimmed}' (${docIds.length}개 말뭉치)가 저장되고 현재 검색 대상으로 선택되었습니다.\n상단 [사용자말뭉치] 메뉴에서도 언제든지 선택해 검색할 수 있습니다.`);
    });
  }

  // 구축 데이터베이스 문서 목록: 선택한 말뭉치에서 바로 검색
  const btnSearchSelectedDocs = document.getElementById('btn-search-selected-docs');
  if (btnSearchSelectedDocs) {
    btnSearchSelectedDocs.addEventListener('click', () => {
      const checkedBoxes = Array.from(document.querySelectorAll('.chk-doc-row:checked'));
      if (checkedBoxes.length === 0) {
        alert('검색할 말뭉치를 목록에서 1개 이상 체크해 주세요.');
        return;
      }
      const docIds = checkedBoxes.map((c) => c.dataset.docId);
      activeUserBundleName = null;
      if (selectUserCorpus) selectUserCorpus.value = '';
      selectedDocIds = new Set(docIds);
      updateDocSelectorTriggerLabel();
      const tabSearchBtn = document.querySelector('.nav-tab[data-tab="tab-search"]');
      if (tabSearchBtn) tabSearchBtn.click();
    });
  }

  // 상단 사용자말뭉치 삭제 버튼
  const btnDeleteActiveUserCorpus = document.getElementById('btn-delete-active-user-corpus');
  if (btnDeleteActiveUserCorpus) {
    btnDeleteActiveUserCorpus.addEventListener('click', () => {
      const val = selectUserCorpus ? selectUserCorpus.value : '';
      if (!val) {
        alert('삭제할 사용자말뭉치를 먼저 선택해 주세요.');
        return;
      }
      if (confirm(`사용자말뭉치 묶음 '${val}'을(를) 삭제하시겠습니까?\n(DB의 원본 말뭉치는 삭제되지 않고 묶음 목록에서만 제거됩니다.)`)) {
        deleteUserCorpusBundle(val);
      }
    });
  }

  // 초기 로드 시 백그라운드 프리패치 및 데이터 로드
  loadOverview();
})();
