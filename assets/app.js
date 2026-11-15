'use strict';

(() => {
  const host = window.chrome && window.chrome.webview;
  const FILE_PREFIX = 'https://mahimarkd.localhost/file/';
  const BIG_DOC = 1500;
  const MAX_HIGHLIGHT = 150000;
  const MAX_MATCHES = 10000;

  const $ = (id) => document.getElementById(id);
  const root = document.documentElement;
  const body = document.body;
  const content = $('content');
  const bar = $('bar');
  const tocNav = $('toc');
  const tocList = $('toc-list');
  const findBox = $('find');
  const findInput = $('find-input');
  const findCount = $('find-count');
  const findCase = $('find-case');
  const exportBtn = $('btn-export');
  const exportMenu = $('export-menu');
  const settingsPane = $('settings');
  const settingsForm = $('settings-form');
  const toastEl = $('toast');

  let S = [];
  let E = [];
  let toc = [];
  let tocKey = '';
  let hasMath = false;
  let settings = { scrollSync: true, clickToSource: true, math: true, showToc: false };
  let appInfo = { name: 'MahiMarkD++', version: '' };
  let editorLine = 0;
  let lastSource = 'editor';
  let userScrollAt = 0;
  let programmaticY = -1;
  let anchorCache = new WeakMap();
  const highlighted = new WeakSet();
  const mathDone = new WeakSet();
  let katexState = 'none';
  let katexWaiters = [];
  let uiFrame = 0;
  let scrollFrame = 0;
  let settleFrame = 0;
  let toastTimer = 0;

  const send = (msg) => {
    if (host) host.postMessage(msg);
  };

  if (window.hljs) {
    window.hljs.configure({ ignoreUnescapedHTML: true, throwUnescapedHTML: false });
  }

  function barHeight() {
    return bar.offsetHeight || 0;
  }

  function topOf(el) {
    return el.getBoundingClientRect().top + window.scrollY;
  }

  function blockAt(i) {
    return content.children[i];
  }

  function lastLE(arr, value) {
    let lo = 0;
    let hi = arr.length - 1;
    let ans = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (arr[mid] <= value) {
        ans = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return ans;
  }

  function anchorsOf(el) {
    let list = anchorCache.get(el);
    if (list) return list;
    list = [{ rel: 0, el }];
    for (const node of el.querySelectorAll('[data-l]')) {
      const rel = Number(node.getAttribute('data-l')) || 0;
      if (rel > list[list.length - 1].rel) list.push({ rel, el: node });
    }
    anchorCache.set(el, list);
    return list;
  }

  function lineToY(line) {
    if (!S.length) return 0;
    const i = lastLE(S, line);
    if (i < 0) return 0;
    const el = blockAt(i);
    if (!el) return 0;
    const base = S[i];
    if (line < E[i] + 1) {
      const anchors = anchorsOf(el);
      let k = 0;
      while (k + 1 < anchors.length && base + anchors[k + 1].rel <= line) k++;
      const la = base + anchors[k].rel;
      const ya = topOf(anchors[k].el);
      let lb;
      let yb;
      if (k + 1 < anchors.length) {
        lb = base + anchors[k + 1].rel;
        yb = topOf(anchors[k + 1].el);
      } else {
        lb = E[i] + 1;
        yb = topOf(el) + el.offsetHeight;
      }
      const f = lb > la ? (line - la) / (lb - la) : 0;
      return ya + Math.min(1, Math.max(0, f)) * Math.max(0, yb - ya);
    }
    const bottom = topOf(el) + el.offsetHeight;
    const next = blockAt(i + 1);
    if (!next) return bottom;
    const gapStart = E[i] + 1;
    const f = (line - gapStart) / Math.max(1, S[i + 1] - gapStart);
    return bottom + Math.min(1, Math.max(0, f)) * (topOf(next) - bottom);
  }

  function yToLine(y) {
    const kids = content.children;
    const n = kids.length;
    if (!n || n !== S.length) return 0;
    let lo = 0;
    let hi = n - 1;
    let i = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (topOf(kids[mid]) <= y) {
        i = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    if (i < 0) return 0;
    const el = kids[i];
    const top = topOf(el);
    const bottom = top + el.offsetHeight;
    if (y >= bottom) {
      if (i + 1 >= n) return E[i] + 1;
      const nextTop = topOf(kids[i + 1]);
      const f = (y - bottom) / Math.max(1, nextTop - bottom);
      return E[i] + 1 + Math.min(1, f) * (S[i + 1] - (E[i] + 1));
    }
    const anchors = anchorsOf(el);
    const base = S[i];
    let k = 0;
    for (let j = 1; j < anchors.length; j++) {
      if (topOf(anchors[j].el) <= y) k = j;
      else break;
    }
    const la = base + anchors[k].rel;
    const ya = topOf(anchors[k].el);
    let lb;
    let yb;
    if (k + 1 < anchors.length) {
      lb = base + anchors[k + 1].rel;
      yb = topOf(anchors[k + 1].el);
    } else {
      lb = E[i] + 1;
      yb = bottom;
    }
    const f = yb > ya ? (y - ya) / (yb - ya) : 0;
    return la + Math.min(1, Math.max(0, f)) * (lb - la);
  }

  function scrollToY(y) {
    const target = Math.max(0, Math.round(y));
    if (Math.abs(window.scrollY - target) < 1) return;
    programmaticY = target;
    window.scrollTo(0, target);
  }

  function scrollToLine(line) {
    if (!S.length || line <= S[0]) {
      cancelAnimationFrame(settleFrame);
      scrollToY(0);
      return;
    }
    scrollToY(lineToY(line) - barHeight());
    cancelAnimationFrame(settleFrame);
    let tries = 0;
    const settle = () => {
      const want = Math.max(0, Math.round(lineToY(line) - barHeight()));
      if (Math.abs(window.scrollY - want) > 2 && tries++ < 3) {
        scrollToY(want);
        settleFrame = requestAnimationFrame(settle);
      }
    };
    settleFrame = requestAnimationFrame(settle);
  }

  function currentLine() {
    return yToLine(window.scrollY + barHeight() + 1);
  }

  function visibleRange(margin) {
    const kids = content.children;
    const n = kids.length;
    if (!n) return [0, -1];
    const from = window.scrollY - margin;
    const to = window.scrollY + window.innerHeight + margin;
    let lo = 0;
    let hi = n - 1;
    let first = 0;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const el = kids[mid];
      if (topOf(el) + el.offsetHeight < from) lo = mid + 1;
      else {
        first = mid;
        hi = mid - 1;
      }
    }
    let last = first;
    while (last + 1 < n && topOf(kids[last + 1]) <= to) last++;
    return [first, last];
  }

  function highlightBlock(el) {
    if (highlighted.has(el)) return;
    highlighted.add(el);
    if (!window.hljs) return;
    for (const code of el.querySelectorAll('pre code[class*="language-"]')) {
      const m = /language-([\w+#.-]+)/.exec(code.className);
      if (!m || !window.hljs.getLanguage(m[1])) continue;
      if (code.textContent.length > MAX_HIGHLIGHT) continue;
      try {
        window.hljs.highlightElement(code);
      } catch (_) {
        // A grammar error leaves the block as plain text.
      }
    }
  }

  function mathBlock(el) {
    if (mathDone.has(el) || katexState !== 'ready' || !window.katex) return;
    mathDone.add(el);
    for (const node of el.querySelectorAll('.math-inline, .math-display')) {
      const tex = node.textContent;
      node.setAttribute('data-tex', tex);
      try {
        window.katex.render(tex, node, { displayMode: node.classList.contains('math-display'), throwOnError: false, output: 'htmlAndMathml' });
      } catch (_) {
        node.textContent = tex;
      }
    }
  }

  function enhance(el) {
    highlightBlock(el);
    if (hasMath) mathBlock(el);
  }

  function enhanceVisible() {
    const [a, b] = visibleRange(window.innerHeight);
    const kids = content.children;
    for (let i = a; i <= b; i++) enhance(kids[i]);
  }

  function loadKatex() {
    if (katexState === 'ready') return Promise.resolve(true);
    if (katexState === 'failed') return Promise.resolve(false);
    const wait = new Promise((resolve) => katexWaiters.push(resolve));
    if (katexState === 'none') {
      katexState = 'loading';
      const css = document.createElement('link');
      css.rel = 'stylesheet';
      css.href = 'vendor/katex/katex.min.css';
      document.head.appendChild(css);
      const js = document.createElement('script');
      js.src = 'vendor/katex/katex.min.js';
      const finish = (ok) => {
        katexState = ok ? 'ready' : 'failed';
        const waiters = katexWaiters;
        katexWaiters = [];
        waiters.forEach((w) => w(ok));
        if (ok) enhanceVisible();
      };
      js.onload = () => finish(true);
      js.onerror = () => finish(false);
      document.head.appendChild(js);
    }
    return wait;
  }

  function setMath(flag) {
    hasMath = !!flag && settings.math !== false;
    if (hasMath) loadKatex();
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  }

  function setToc(list) {
    toc = list || [];
    const key = toc.map((t) => t.level + t.id + t.text).join('\u0000');
    if (key === tocKey) return;
    tocKey = key;
    if (!toc.length) {
      tocList.innerHTML = '<li class="toc-empty">No headings</li>';
      return;
    }
    const minLevel = Math.min(...toc.slice(0, 2000).map((t) => t.level));
    tocList.innerHTML = toc
      .map((t, i) => `<li class="l${Math.min(6, t.level - minLevel + 1)}"><a href="#${escapeHtml(t.id)}" data-i="${i}" title="${escapeHtml(t.text)}">${escapeHtml(t.text) || '&nbsp;'}</a></li>`)
      .join('');
    updateTocActive();
  }

  function updateTocActive() {
    if (!body.classList.contains('toc-open') || !toc.length) return;
    const line = currentLine();
    let idx = -1;
    for (let i = 0; i < toc.length; i++) {
      if (toc[i].line <= line + 0.5) idx = i;
      else break;
    }
    const prev = tocList.querySelector('a.active');
    const next = idx >= 0 ? tocList.querySelector(`a[data-i="${idx}"]`) : null;
    if (prev === next) return;
    if (prev) prev.classList.remove('active');
    if (next) {
      next.classList.add('active');
      const r = next.getBoundingClientRect();
      const nr = tocNav.getBoundingClientRect();
      if (r.top < nr.top || r.bottom > nr.bottom) next.scrollIntoView({ block: 'nearest' });
    }
  }

  function setTocOpen(open, persist) {
    body.classList.toggle('toc-open', open);
    tocNav.hidden = !open;
    $('btn-toc').setAttribute('aria-pressed', String(open));
    if (open) updateTocActive();
    if (persist) send({ t: 'toc', show: open });
  }

  function afterContentChange() {
    content.classList.toggle('big', S.length > BIG_DOC);
    if (settings.scrollSync && lastSource === 'editor') scrollToLine(editorLine);
    scheduleUi();
    if (!findBox.hidden && findInput.value) runFind(false);
  }

  function applyFull(m) {
    anchorCache = new WeakMap();
    content.innerHTML = m.html;
    S = m.s;
    E = m.e;
    lastSource = 'editor';
    setToc(m.toc);
    setMath(m.math);
    if (!S.length) content.innerHTML = '<div id="empty">Nothing to preview yet.</div>';
    afterContentChange();
  }

  function spliceArray(arr, at, remove, items) {
    if (items.length < 20000 && remove < 20000) {
      arr.splice(at, remove, ...items);
      return arr;
    }
    return arr.slice(0, at).concat(items, arr.slice(at + remove));
  }

  function applyPatch(m) {
    const empty = $('empty');
    if (empty) empty.remove();
    const kids = content.children;
    for (let k = 0; k < m.r; k++) {
      const el = kids[m.p];
      if (el) el.remove();
    }
    if (m.html) {
      const tpl = document.createElement('template');
      tpl.innerHTML = m.html;
      content.insertBefore(tpl.content, kids[m.p] || null);
    }
    S = spliceArray(S, m.p, m.r, m.s);
    E = spliceArray(E, m.p, m.r, m.e);
    const midStart = m.p;
    const midEnd = m.p + m.s.length;
    const runs = m.shift || [];
    for (let k = 0; k < runs.length; k++) {
      const [from, ds, de] = runs[k];
      if (!ds && !de) continue;
      const to = k + 1 < runs.length ? runs[k + 1][0] : S.length;
      for (let i = from; i < to; i++) {
        if (i >= midStart && i < midEnd) continue;
        S[i] += ds;
        E[i] += de;
      }
    }
    if (m.toc) setToc(m.toc);
    setMath(m.math);
    if (!S.length) content.innerHTML = '<div id="empty">Nothing to preview yet.</div>';
    afterContentChange();
  }

  function scheduleUi() {
    if (uiFrame) return;
    uiFrame = requestAnimationFrame(() => {
      uiFrame = 0;
      enhanceVisible();
      updateTocActive();
    });
  }

  window.addEventListener(
    'scroll',
    () => {
      if (programmaticY >= 0 && Math.abs(window.scrollY - programmaticY) <= 2) {
        programmaticY = -1;
        scheduleUi();
        return;
      }
      programmaticY = -1;
      userScrollAt = performance.now();
      lastSource = 'user';
      if (!scrollFrame) {
        scrollFrame = requestAnimationFrame(() => {
          scrollFrame = 0;
          editorLine = currentLine();
          if (settings.scrollSync) send({ t: 'scroll', line: editorLine });
        });
      }
      scheduleUi();
    },
    { passive: true },
  );

  window.addEventListener('resize', () => {
    if (settings.scrollSync && lastSource === 'editor') scrollToLine(editorLine);
    scheduleUi();
  });

  new ResizeObserver(() => {
    if (settings.scrollSync && lastSource === 'editor' && performance.now() - userScrollAt > 300) {
      const want = Math.round(lineToY(editorLine) - barHeight());
      if (Math.abs(window.scrollY - want) > 2) scrollToY(want);
    }
  }).observe(content);

  function onEditorScroll(line) {
    if (performance.now() - userScrollAt < 250) return;
    editorLine = line;
    lastSource = 'editor';
    if (settings.scrollSync) scrollToLine(line);
  }

  function jumpToId(id) {
    const el = document.getElementById(id);
    if (!el || !content.contains(el)) return;
    const block = el.closest('.b');
    if (block) block.scrollIntoView({ block: 'start' });
    requestAnimationFrame(() => {
      window.scrollTo(0, Math.max(0, topOf(el) - barHeight() - 8));
    });
  }

  async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_) {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      return ok;
    }
  }

  function toast(text, error) {
    toastEl.textContent = text;
    toastEl.classList.toggle('error', !!error);
    toastEl.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toastEl.classList.remove('show'), error ? 5000 : 2600);
  }

  let selectedMath = [];

  function mathInRange(range) {
    const anc = range.commonAncestorContainer;
    const scope = anc.nodeType === 1 ? anc : anc.parentElement;
    if (!scope) return [];
    const own = scope.closest('.math-inline[data-tex], .math-display[data-tex]');
    if (own) return [own];
    return [...scope.querySelectorAll('.math-inline[data-tex], .math-display[data-tex]')].filter((n) => range.intersectsNode(n));
  }

  document.addEventListener('selectionchange', () => {
    const sel = window.getSelection();
    const next = sel && sel.rangeCount && !sel.isCollapsed ? mathInRange(sel.getRangeAt(0)) : [];
    for (const n of selectedMath) if (!next.includes(n)) n.classList.remove('sel');
    for (const n of next) n.classList.add('sel');
    selectedMath = next;
  });

  document.addEventListener('copy', (e) => {
    const sel = window.getSelection();
    if (!sel || !sel.rangeCount || sel.isCollapsed || !content.contains(sel.anchorNode)) return;
    const range = sel.getRangeAt(0);
    if (!mathInRange(range).length) return;
    const holder = document.createElement('div');
    holder.appendChild(range.cloneContents());
    const own = range.commonAncestorContainer;
    const ownMath = (own.nodeType === 1 ? own : own.parentElement).closest('.math-inline[data-tex], .math-display[data-tex]');
    if (ownMath) {
      holder.textContent = '';
      holder.appendChild(ownMath.cloneNode(true));
    }
    for (const n of holder.querySelectorAll('.math-inline[data-tex], .math-display[data-tex]')) {
      const display = n.classList.contains('math-display');
      n.replaceWith(display ? `$$${n.getAttribute('data-tex')}$$` : `$${n.getAttribute('data-tex')}$`);
    }
    for (const n of holder.querySelectorAll('.katex')) n.replaceWith(n.querySelector('annotation') ? n.querySelector('annotation').textContent : '');
    for (const n of holder.querySelectorAll('.code-bar, a.anchor')) n.remove();
    holder.style.cssText = 'position:fixed;left:-99999px;top:0;white-space:pre-wrap';
    body.appendChild(holder);
    const text = holder.innerText;
    holder.remove();
    e.clipboardData.setData('text/plain', text);
    e.preventDefault();
  });

  function closeMenus() {
    exportMenu.hidden = true;
    exportBtn.setAttribute('aria-expanded', 'false');
  }

  document.addEventListener('click', (e) => {
    const target = e.target instanceof Element ? e.target : null;
    if (!target) return;

    if (!target.closest('.menu-wrap')) closeMenus();

    const copyBtn = target.closest('.code-copy');
    if (copyBtn) {
      e.preventDefault();
      const code = copyBtn.closest('.code');
      const pre = code && code.querySelector('pre');
      if (!pre) return;
      copyText(pre.textContent.replace(/\n$/, '')).then((ok) => {
        copyBtn.classList.toggle('copied', ok);
        copyBtn.title = ok ? 'Copied' : 'Copy failed';
        setTimeout(() => {
          copyBtn.classList.remove('copied');
          copyBtn.title = 'Copy code';
        }, 1600);
      });
      return;
    }

    const anchor = target.closest('a.anchor');
    if (anchor && content.contains(anchor)) {
      e.preventDefault();
      const href = anchor.getAttribute('href') || '';
      copyText(href).then((ok) => toast(ok ? `Copied link ${href}` : 'Copy failed', !ok));
      return;
    }

    const link = target.closest('a[href]');
    if (link && (content.contains(link) || tocNav.contains(link))) {
      e.preventDefault();
      const href = link.getAttribute('href') || '';
      if (href.startsWith('#')) {
        let id = href.slice(1);
        try {
          id = decodeURIComponent(id);
        } catch (_) {
          // Keep the raw id.
        }
        jumpToId(id);
      } else if (href) {
        send({ t: 'link', href });
      }
      return;
    }
    if (link) {
      e.preventDefault();
      return;
    }

    if (
      settings.clickToSource &&
      e.detail === 1 &&
      content.contains(target) &&
      !target.closest('button, input, summary, select, textarea, .katex') &&
      window.getSelection().isCollapsed
    ) {
      const line = yToLine(e.clientY + window.scrollY);
      send({ t: 'goto', line: Math.max(0, Math.floor(line)) });
    }
  });

  $('btn-toc').addEventListener('click', () => setTocOpen(!body.classList.contains('toc-open'), true));
  $('btn-find').addEventListener('click', () => openFind());
  $('btn-settings').addEventListener('click', () => openSettings());
  exportBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    const open = exportMenu.hidden;
    exportMenu.hidden = !open;
    exportBtn.setAttribute('aria-expanded', String(open));
  });
  exportMenu.addEventListener('click', (e) => {
    const item = e.target instanceof Element ? e.target.closest('[data-kind]') : null;
    if (!item) return;
    closeMenus();
    send({ t: 'export', kind: item.getAttribute('data-kind') });
  });

  const find = { matches: [], index: -1 };

  function openFind() {
    findBox.hidden = false;
    const sel = String(window.getSelection() || '').trim();
    if (sel && !sel.includes('\n') && sel.length < 200) findInput.value = sel;
    findInput.focus();
    findInput.select();
    if (findInput.value) runFind(true);
  }

  function closeFind() {
    findBox.hidden = true;
    findBox.classList.remove('none');
    if (window.CSS && CSS.highlights) {
      CSS.highlights.delete('find');
      CSS.highlights.delete('find-current');
    }
    find.matches = [];
    find.index = -1;
  }

  function runFind(jump) {
    const q = findInput.value;
    const matchCase = findCase.getAttribute('aria-pressed') === 'true';
    find.matches = [];
    find.index = -1;
    if (window.CSS && CSS.highlights) {
      CSS.highlights.delete('find');
      CSS.highlights.delete('find-current');
    }
    if (!q) {
      findCount.textContent = '';
      findBox.classList.remove('none');
      return;
    }
    const needle = matchCase ? q : q.toLowerCase();
    const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (n.parentElement && n.parentElement.closest('.katex-mathml, .code-bar') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
    });
    let node;
    while ((node = walker.nextNode()) && find.matches.length < MAX_MATCHES) {
      const raw = node.data;
      const text = matchCase ? raw : raw.toLowerCase();
      if (text.length !== raw.length) continue;
      let pos = text.indexOf(needle);
      while (pos !== -1 && find.matches.length < MAX_MATCHES) {
        const r = new Range();
        r.setStart(node, pos);
        r.setEnd(node, pos + needle.length);
        find.matches.push(r);
        pos = text.indexOf(needle, pos + needle.length);
      }
    }
    if (window.CSS && CSS.highlights && find.matches.length) {
      CSS.highlights.set('find', new Highlight(...find.matches));
    }
    findBox.classList.toggle('none', !find.matches.length);
    if (!find.matches.length) {
      findCount.textContent = 'No results';
      return;
    }
    const top = window.scrollY + barHeight();
    let idx = find.matches.findIndex((r) => {
      const rect = r.getBoundingClientRect();
      return rect.height > 0 && rect.top + window.scrollY >= top;
    });
    if (idx < 0) idx = 0;
    selectMatch(idx, jump);
  }

  function selectMatch(idx, jump) {
    const n = find.matches.length;
    if (!n) return;
    find.index = ((idx % n) + n) % n;
    const r = find.matches[find.index];
    if (window.CSS && CSS.highlights) CSS.highlights.set('find-current', new Highlight(r));
    findCount.textContent = `${find.index + 1} of ${n}${n >= MAX_MATCHES ? '+' : ''}`;
    if (!jump) return;
    const block = r.startContainer.parentElement && r.startContainer.parentElement.closest('.b');
    const reveal = () => {
      const rect = r.getBoundingClientRect();
      const viewTop = barHeight();
      if (rect.top < viewTop + 20 || rect.bottom > window.innerHeight - 20) {
        window.scrollTo(0, Math.max(0, rect.top + window.scrollY - window.innerHeight / 3));
      }
    };
    if (block && block.getBoundingClientRect().height === 0) {
      block.scrollIntoView({ block: 'center' });
      requestAnimationFrame(reveal);
    } else {
      reveal();
    }
  }

  findInput.addEventListener('input', () => runFind(true));
  findInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      selectMatch(find.index + (e.shiftKey ? -1 : 1), true);
    }
  });
  findCase.addEventListener('click', () => {
    findCase.setAttribute('aria-pressed', String(findCase.getAttribute('aria-pressed') !== 'true'));
    runFind(true);
    findInput.focus();
  });
  $('find-next').addEventListener('click', () => selectMatch(find.index + 1, true));
  $('find-prev').addEventListener('click', () => selectMatch(find.index - 1, true));
  $('find-close').addEventListener('click', closeFind);

  function fillSettings() {
    for (const el of settingsForm.elements) {
      if (!el.name || !(el.name in settings)) continue;
      if (el.type === 'checkbox') el.checked = !!settings[el.name];
      else if (document.activeElement !== el) el.value = settings[el.name];
    }
    $('settings-foot').textContent = `${appInfo.name} ${appInfo.version} · Settings are saved automatically.`;
  }

  function openSettings() {
    closeMenus();
    fillSettings();
    settingsPane.hidden = false;
    const first = settingsForm.querySelector('select, input');
    if (first) first.focus();
  }

  function closeSettings() {
    settingsPane.hidden = true;
  }

  settingsForm.addEventListener('change', (e) => {
    const el = e.target;
    if (!el.name) return;
    let value;
    if (el.type === 'checkbox') value = el.checked;
    else if (el.type === 'number') value = Number(el.value) || 0;
    else value = el.value;
    send({ t: 'settings', value: Object.assign({}, settings, { [el.name]: value }) });
  });
  settingsForm.addEventListener('submit', (e) => e.preventDefault());
  $('settings-close').addEventListener('click', closeSettings);
  settingsPane.addEventListener('click', (e) => {
    if (e.target === settingsPane) closeSettings();
  });

  document.addEventListener('keydown', (e) => {
    const ctrl = e.ctrlKey || e.metaKey;
    if (ctrl && !e.shiftKey && !e.altKey && (e.key === 'f' || e.key === 'F')) {
      e.preventDefault();
      openFind();
    } else if (e.key === 'F3') {
      e.preventDefault();
      if (findBox.hidden) openFind();
      else selectMatch(find.index + (e.shiftKey ? -1 : 1), true);
    } else if (e.key === 'Escape') {
      if (!exportMenu.hidden) closeMenus();
      else if (!settingsPane.hidden) closeSettings();
      else if (!findBox.hidden) closeFind();
      else send({ t: 'focusEditor' });
    } else if (ctrl && (e.key === '=' || e.key === '+')) {
      e.preventDefault();
      send({ t: 'zoom', op: 'in' });
    } else if (ctrl && e.key === '-') {
      e.preventDefault();
      send({ t: 'zoom', op: 'out' });
    } else if (ctrl && e.key === '0') {
      e.preventDefault();
      send({ t: 'zoom', op: 'reset' });
    }
  });

  function applyConfig(m) {
    const first = !appInfo.version;
    settings = m.settings || settings;
    appInfo = { name: m.name || appInfo.name, version: m.version || '' };
    const theme = m.theme || {};
    root.setAttribute('data-theme', theme.dark ? 'dark' : 'light');
    if (theme.editor) {
      root.style.setProperty('--bg', theme.bg);
      root.style.setProperty('--fg', theme.fg);
    } else {
      root.style.removeProperty('--bg');
      root.style.removeProperty('--fg');
    }
    root.style.setProperty('--font-size', `${settings.fontSize || 15}px`);
    if (settings.fontFamily) root.style.setProperty('--font-body', `${settings.fontFamily}, -apple-system, "Segoe UI", sans-serif`);
    else root.style.removeProperty('--font-body');
    if (settings.codeFontFamily) root.style.setProperty('--font-code', `${settings.codeFontFamily}, ui-monospace, Consolas, monospace`);
    else root.style.removeProperty('--font-code');
    root.style.setProperty('--content-width', settings.contentWidth ? `${settings.contentWidth}px` : 'none');
    if (first) setTocOpen(!!settings.showToc, false);
    if (settings.math === false) hasMath = false;
    fillSettings();
    anchorCache = new WeakMap();
    scheduleUi();
  }

  function waitImages(scope, timeoutMs) {
    const pending = [];
    for (const img of scope.querySelectorAll('img')) {
      img.loading = 'eager';
      if (!img.complete) {
        pending.push(
          new Promise((resolve) => {
            img.addEventListener('load', resolve, { once: true });
            img.addEventListener('error', resolve, { once: true });
          }),
        );
      }
    }
    if (!pending.length) return Promise.resolve();
    return Promise.race([Promise.all(pending), new Promise((r) => setTimeout(r, timeoutMs))]);
  }

  async function enhanceAll() {
    if (hasMath) await loadKatex();
    for (const el of content.children) enhance(el);
  }

  function toFileUri(src) {
    const rest = src.slice(FILE_PREFIX.length);
    return rest.startsWith('//') ? `file:${rest}` : `file:///${rest}`;
  }

  async function inlineKatexCss() {
    let css = await (await fetch('vendor/katex/katex.min.css')).text();
    const fonts = [...new Set(css.match(/fonts\/[\w-]+\.woff2/g) || [])];
    for (const f of fonts) {
      const buf = await (await fetch(`vendor/katex/${f}`)).arrayBuffer();
      let bin = '';
      const bytes = new Uint8Array(buf);
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
      css = css.split(`url(${f})`).join(`url(data:font/woff2;base64,${btoa(bin)})`);
    }
    return css;
  }

  async function exportHtml(m) {
    try {
      await enhanceAll();
      const clone = content.cloneNode(true);
      clone.classList.remove('big');
      clone.removeAttribute('id');
      clone.querySelectorAll('.code-copy, a.anchor, #empty').forEach((n) => n.remove());
      clone.querySelectorAll('[data-l]').forEach((n) => n.removeAttribute('data-l'));
      clone.querySelectorAll('img').forEach((img) => {
        const src = img.getAttribute('src') || '';
        if (src.startsWith(FILE_PREFIX)) {
          const orig = img.getAttribute('data-orig');
          img.setAttribute('src', m.sameDir && orig ? orig : toFileUri(src));
        }
        img.removeAttribute('data-orig');
      });
      clone.querySelectorAll('div.b').forEach((b) => b.replaceWith(...b.childNodes));
      const docCss = await (await fetch('doc.css')).text();
      const katexCss = clone.querySelector('.katex') ? await inlineKatexCss() : '';
      const page = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="${escapeHtml(appInfo.name)} ${escapeHtml(appInfo.version)}">
<title>${escapeHtml(m.title || 'Document')}</title>
<style>
${docCss}
${katexCss}
.markdown-body{padding:32px 28px 48px}
.markdown-body .code-bar{display:none}
.markdown-body .code>pre{padding-top:14px;border-radius:8px}
</style>
</head>
<body>
<article class="markdown-body">
${clone.innerHTML}
</article>
</body>
</html>
`;
      send({ t: 'exportHtml', id: m.id, html: page });
    } catch (err) {
      toast(`HTML export failed: ${err && err.message ? err.message : err}`, true);
    }
  }

  async function preparePrint(m) {
    try {
      await enhanceAll();
      body.classList.add('printing');
      await waitImages(content, 8000);
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      send({ t: 'printReady', id: m.id });
    } catch (err) {
      body.classList.remove('printing');
      toast(`PDF export failed: ${err && err.message ? err.message : err}`, true);
    }
  }

  if (host) {
    host.addEventListener('message', (e) => {
      const m = e.data;
      if (!m || typeof m !== 'object') return;
      switch (m.t) {
        case 'config':
          applyConfig(m);
          break;
        case 'full':
          applyFull(m);
          break;
        case 'patch':
          applyPatch(m);
          break;
        case 'scroll':
          onEditorScroll(m.line);
          break;
        case 'toast':
          toast(m.text, m.error);
          break;
        case 'openSettings':
          openSettings();
          break;
        case 'exportHtml':
          exportHtml(m);
          break;
        case 'preparePrint':
          preparePrint(m);
          break;
        case 'printDone':
          body.classList.remove('printing');
          break;
        default:
          break;
      }
    });
  }

  send({ t: 'ready' });
})();
