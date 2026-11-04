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
  const selectLayer = document.getElementById('select-layer');
  const selectDoc = document.getElementById('select-doc');
  const selectContextWords = document.getElementById('select-context-words');
  const selectLimit = document.getElementById('select-limit');
  const selectCorpusFont = document.getElementById('select-corpus-font');

  // Search Options Collapsible Bar
  const btnToggleSearchOptions = document.getElementById('btn-toggle-search-options');
  const searchOptionsBar = document.getElementById('search-options-bar');
  const foldIcon = document.getElementById('fold-icon');
  const foldText = document.getElementById('fold-text');

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
      btnFoldToggleBtn.textContent = '접기 ▴';
    } else {
      inspectorFoldCard.classList.remove('expanded');
      inspectorFoldArrow.textContent = '▶';
      btnFoldToggleBtn.textContent = '펼치기 ▾';
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
      } else if (kind === 'V') {
        automata.feedVowel(val);
      } else if (kind === 'TONE') {
        automata.feedTone(val);
      } else if (kind === 'ALONE') {
        automata.feedStandaloneVowel(val);
      }
      searchInput.value = automata.getText();
      updateKbStatus();
    }
  });

  searchInput.addEventListener('input', () => {
    if (oldHangulImeActive) {
      searchInput.value = automata.getText();
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
      selectMode.value = mode;
      selectLayer.value = layer;
      selectDoc.value = 'ALL';
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

      selectDoc.innerHTML = '<option value="ALL">전체 문서 (13종)</option>';
      data.documents.forEach((d) => {
        const opt = document.createElement('option');
        opt.value = d.doc_id;
        opt.textContent = `[${d.doc_id}] ${d.title} (${d.sentence_count.toLocaleString()}문장)`;
        selectDoc.appendChild(opt);
      });

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

      const docsTbody = document.getElementById('docs-tbody');
      docsTbody.innerHTML = data.documents
        .map(
          (d) => `
        <tr>
          <td><code><strong>${escapeHtml(d.doc_id)}</strong></code></td>
          <td style="font-weight:700;">${escapeHtml(d.title)}</td>
          <td>${escapeHtml(d.corpus_category)}</td>
          <td><span class="layer-badge badge-syntax">${escapeHtml(d.annotation_layers)}</span></td>
          <td><code>${escapeHtml(d.source_files_list)}</code></td>
          <td style="text-align:right; font-weight:700;">${d.sentence_count.toLocaleString()}</td>
          <td style="text-align:right;">${d.eojeol_count.toLocaleString()}</td>
          <td>
            <button type="button" class="btn btn-primary btn-browse-doc" data-doc="${escapeHtml(d.doc_id)}" style="padding:4px 10px; font-size:0.76rem;">
              📖 이 문서 열람
            </button>
          </td>
        </tr>
      `
        )
        .join('');

      document.querySelectorAll('.btn-browse-doc').forEach((b) => {
        b.addEventListener('click', () => {
          const docId = b.dataset.doc;
          document.querySelector('.nav-tab[data-tab="tab-search"]').click();
          searchInput.value = '';
          automata.reset();
          selectDoc.value = docId;
          selectLayer.value = 'ALL';
          selectMode.value = 'all';
          runSearch();
        });
      });

      // Run initial showcase search (사용자가 이미 입력 중인 경우 덮어쓰지 않음)
      if (!searchInput.value.trim() && document.activeElement !== searchInput) {
        searchInput.value = '세계__02';
        selectMode.value = 'all';
        selectLayer.value = 'ALL';
        runSearch();
      }
    } catch (err) {
      console.error('Overview load error:', err);
    }
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
    selectMode.value = tab.mode;
    selectLayer.value = tab.layer;
    selectDoc.value = tab.docId;
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
    kwicResultTitle.textContent = tab.query
      ? `용례 검색 결과: "${tab.query}" (${ctxLabel})`
      : `문서 열람 (${tab.docId !== 'ALL' ? tab.docId : tab.layer} · ${ctxLabel})`;

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

    kwicTbody.innerHTML = visibleHits
      .map((h) => {
        const badges = [];
        if (h.corpus_category.includes('옛한글')) {
          badges.push('<span class="layer-badge badge-old">옛한글</span>');
        }
        if (h.has_morph) {
          badges.push('<span class="layer-badge badge-morph">형태</span>');
        }
        if (h.has_sense) {
          badges.push('<span class="layer-badge badge-sense">의미</span>');
        }
        if (h.has_syntax) {
          badges.push('<span class="layer-badge badge-syntax">구문</span>');
        }
        const loc = h.page_ref
          ? `${h.doc_id} (${h.page_ref})`
          : `${h.doc_id} #${h.seq_no}`;

        return `
          <tr class="kwic-row" data-seg-id="${h.segment_id}" data-text="${escapeHtml(h.text_unicode)}" title="클릭: 상세 뷰어 / 더블클릭: 뷰어 펼치기 / 마우스 우클릭: 원본 파일 점프">
            <td>
              <div style="font-weight:700; font-size:0.76rem; color:#1e293b;">${escapeHtml(loc)}</div>
              <div style="font-size:0.7rem; color:#64748b;">[${escapeHtml(h.match_layer)}]</div>
            </td>
            <td>${badges.join('')}</td>
            <td class="kwic-left">${escapeHtml(h.left_context)}</td>
            <td class="kwic-node-cell"><span class="kwic-node">${escapeHtml(h.node || '·')}</span></td>
            <td class="kwic-right">${escapeHtml(h.right_context)}</td>
          </tr>
        `;
      })
      .join('');

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

  // 3. Search Result Export (텍스트 파일 저장 기능)
  btnExportKwicTxt.addEventListener('click', () => {
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
      '원문(유니코드)',
      '형태소분석',
      '형태의미분석',
      '구문분석',
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
    const blob = new Blob([fileContent], { type: 'text/tab-separated-values;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    const safeTitle = (tab.title || 'results').replace(/[\\/:*?"<>|]/g, '_');
    const dateStr = new Date().toISOString().slice(0, 10);
    a.href = url;
    a.download = `concordance_${safeTitle}_${dateStr}.txt`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
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

  // Search Options Bar Fold/Unfold Toggle
  if (btnToggleSearchOptions && searchOptionsBar) {
    const applySearchOptionsFold = (collapsed) => {
      if (collapsed) {
        searchOptionsBar.classList.add('collapsed');
        if (foldIcon) foldIcon.textContent = '🔽';
        if (foldText) foldText.textContent = '검색창 상단 옵션 펼치기';
      } else {
        searchOptionsBar.classList.remove('collapsed');
        if (foldIcon) foldIcon.textContent = '🔼';
        if (foldText) foldText.textContent = '검색창 상단 옵션 접기';
      }
    };

    // Restore saved state from localStorage
    const savedCollapsed = localStorage.getItem('sconc_search_options_collapsed') === '1';
    if (savedCollapsed) {
      applySearchOptionsFold(true);
    }

    btnToggleSearchOptions.addEventListener('click', () => {
      const isCurrentlyCollapsed = searchOptionsBar.classList.contains('collapsed');
      const nextCollapsed = !isCurrentlyCollapsed;
      applySearchOptionsFold(nextCollapsed);
      if (nextCollapsed) {
        localStorage.setItem('sconc_search_options_collapsed', '1');
      } else {
        localStorage.removeItem('sconc_search_options_collapsed');
      }
    });
  }

  // 2. Folder-based DB Builder
  btnStartBuild.addEventListener('click', async () => {
    const folderPath = builderFolderPath.value.trim();
    const dbName = builderDbName.value.trim();
    if (!folderPath) {
      alert('말뭉치 폴더 경로를 입력해 주세요.');
      builderFolderPath.focus();
      return;
    }

    btnStartBuild.disabled = true;
    builderStatusBadge.style.display = 'inline-block';
    builderStatusBadge.innerHTML = '⏳ 지정한 폴더 내 파일 형식 분석 및 데이터베이스 생성 중… (수 초~수십 초 소요)';
    builderResultArea.style.display = 'none';

    try {
      const res = await fetch('/api/build-db', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ folder_path: folderPath, db_name: dbName }),
      });
      const data = await res.json();
      btnStartBuild.disabled = false;
      builderStatusBadge.style.display = 'none';

      if (data.error) {
        alert(`데이터베이스 구축 오류:\n${data.error}`);
        return;
      }

      builderSummaryBanner.innerHTML = `
        <strong>🎉 데이터베이스 생성 완료!</strong><br/>
        폴더: <code>${escapeHtml(data.folder)}</code> → 대상 DB: <code>${escapeHtml(data.db_path)}</code><br/>
        스캔 파일: <strong>${data.total_files_scanned}개</strong> · 신규 통합 문서: <strong>${data.documents_created}개</strong> · 추출 문장: <strong>${(data.sentences_imported || 0).toLocaleString()}개</strong> (${(data.eojeols_imported || 0).toLocaleString()}어절)
      `;

      builderItemsTbody.innerHTML = (data.items || [])
        .map(
          (item) => `
        <tr>
          <td><code><strong>${escapeHtml(item.doc_id)}</strong></code></td>
          <td><span class="layer-badge badge-old">${escapeHtml(item.type)}</span></td>
          <td><code>${escapeHtml((item.files || []).join(', '))}</code></td>
          <td style="text-align:right; font-weight:700;">${(item.sentences || 0).toLocaleString()}문장</td>
        </tr>
      `
        )
        .join('');

      builderResultArea.style.display = 'block';

      // Reload overview to reflect newly added documents in dropdowns and tables
      await loadOverview();
    } catch (err) {
      btnStartBuild.disabled = false;
      builderStatusBadge.style.display = 'none';
      alert(`데이터베이스 생성 요청 실패: ${err.message}`);
    }
  });

  btnJumpToSearch.addEventListener('click', () => {
    document.querySelector('.nav-tab[data-tab="tab-search"]').click();
    searchInput.value = '';
    automata.reset();
    selectDoc.value = 'ALL';
    runSearch();
  });

  async function runSearch() {
    // 1. 선행 파라미터 안전 조회 (TDZ 방지)
    const q = searchInput.value;
    const mode = selectMode.value;
    const layer = selectLayer.value;
    const docId = selectDoc.value;
    const contextWords = selectContextWords.value;
    const limit = selectLimit.value;

    if (!q.trim() && docId === 'ALL') {
      kwicResultMeta.innerHTML = '<span style="color:#64748b;">검색어를 입력하거나 특정 문서를 선택해 주세요.</span>';
      return;
    }

    // 2. 직전 검색이 진행 중인 경우 조용히 취소 (새 검색으로 인한 정상 교체)
    searchExplicitlyCancelled = false;
    if (currentSearchAbort) {
      try {
        currentSearchAbort.abort();
      } catch (_) {}
      currentSearchAbort = null;
    }

    // 3. 고유 시퀀스 번호 및 AbortController 발급
    const mySearchSeq = ++currentSearchSeq;
    const myAbortController = new AbortController();
    currentSearchAbort = myAbortController;
    isSearchInProgress = true;

    // UI 상태 반영 (취소 버튼 노출 및 검색 버튼 비활성화)
    btnCancelSearch.style.display = 'inline-flex';
    if (btnSearch) {
      btnSearch.disabled = true;
      btnSearch.innerHTML = '⏳ 검색 중...';
    }
    kwicResultMeta.innerHTML = '<strong style="color:#2563eb;">⏳ 검색 진행 중… (우측 [⏹ 검색 종료] 버튼으로 중단 가능)</strong>';

    const qs = new URLSearchParams({
      q,
      mode,
      layer_filter: layer,
      doc_id: docId,
      context_words: contextWords,
      limit,
    });

    try {
      const res = await fetch(`/api/search?${qs.toString()}`, {
        signal: myAbortController.signal,
      });
      const data = await res.json();

      // 만약 응답 도착 시점에 더 최신의 새 검색이 시작되었다면 이 결과는 폐기
      if (mySearchSeq !== currentSearchSeq) {
        return;
      }

      isSearchInProgress = false;
      btnCancelSearch.style.display = 'none';
      if (btnSearch) {
        btnSearch.disabled = false;
        btnSearch.innerHTML = '🔍 검색';
      }
      currentSearchAbort = null;

      if (data.error) {
        kwicResultMeta.innerHTML = `<strong style="color:#dc2626;">오류: ${escapeHtml(data.error)}</strong>`;
        return;
      }

      // Multi-Tab management: Add new tab (up to 10 tabs)
      const tabTitle = q || (docId !== 'ALL' ? docId : '전체 열람');
      // If active tab exists and had empty query and no results, reuse it; else add new tab
      let targetTab = null;
      if (searchTabs.length > 0 && activeTabId) {
        const curTab = searchTabs.find((t) => t.id === activeTabId);
        if (curTab && !curTab.query && (!curTab.data || !curTab.data.hits || curTab.data.hits.length === 0)) {
          targetTab = curTab;
        }
      }

      if (targetTab) {
        targetTab.query = q;
        targetTab.title = tabTitle;
        targetTab.mode = mode;
        targetTab.layer = layer;
        targetTab.docId = docId;
        targetTab.contextWords = contextWords;
        targetTab.limit = limit;
        targetTab.data = data;
        targetTab.subsearchQuery = '';
        targetTab.selectedSegId = data.hits && data.hits[0] ? data.hits[0].segment_id : null;
      } else {
        if (searchTabs.length >= 10) {
          searchTabs.shift(); // Max 10 tabs: remove oldest
        }
        const newTab = {
          id: 'tab_' + Date.now() + '_' + Math.random().toString(36).slice(2, 6),
          query: q,
          title: tabTitle,
          mode,
          layer,
          docId,
          contextWords,
          limit,
          data,
          subsearchQuery: '',
          selectedSegId: data.hits && data.hits[0] ? data.hits[0].segment_id : null,
        };
        searchTabs.push(newTab);
        activeTabId = newTab.id;
      }

      renderTabsBar();
      renderActiveTabHits();
    } catch (err) {
      // 최신 검색이 아닌 이전 검색의 Abort/Error라면 현재 화면을 전혀 건드리지 않고 조용히 폐기
      if (mySearchSeq !== currentSearchSeq) {
        return;
      }

      isSearchInProgress = false;
      btnCancelSearch.style.display = 'none';
      if (btnSearch) {
        btnSearch.disabled = false;
        btnSearch.innerHTML = '🔍 검색';
      }

      if (err.name === 'AbortError') {
        // 사용자가 [⏹ 검색 종료] 버튼을 직접 눌렀을 때만 중단 메시지 표시
        if (searchExplicitlyCancelled) {
          kwicResultMeta.innerHTML = '<strong style="color:#dc2626;">⏹ 검색이 사용자에 의해 중단되었습니다.</strong>';
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
                <div class="eojeol-surface">${escapeHtml(ej.surface || '')} <small style="color:#64748b; font-weight:400;">${escapeHtml(ej.ej_id || '')}</small></div>
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
  const btnFillSampleFolder = document.getElementById('btn-fill-sample-folder');
  if (btnFillSampleFolder) {
    btnFillSampleFolder.addEventListener('click', () => {
      if (builderFolderPath) builderFolderPath.value = 'c:\\AG\\concordance\\sample';
      if (builderDbName) builderDbName.value = 'concordance.corpdb';
    });
  }

  loadOverview();
  pollBuildStatus();
})();
