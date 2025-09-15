(function(){
  function classifyStatus(status){
    const s = (status || '').toLowerCase().trim();
    if (s.startsWith('planned')) return 'planned';
    if (s.startsWith('experimental')) return 'experimental';
    if (s.startsWith('beta')) return 'beta';
    if (s.startsWith('stable')) return 'stable';
    if (s.startsWith('deprecated')) return 'deprecated';
    return 'unknown';
  }

  function makeBadge(kind, value){
    const span = document.createElement('span');
    span.className = 'badge';
    if (kind === 'status'){
      span.classList.add('status-badge', 'status-' + classifyStatus(value));
      span.textContent = 'Status: ' + value;
    } else if (kind === 'updated'){
      span.classList.add('updated-badge');
      span.textContent = 'Updated: ' + value;
      span.setAttribute('data-date', value);
    } else if (kind === 'type'){
      // Diátaxis type
      span.classList.add('type-badge');
      span.textContent = 'Type: ' + value;
    }
    return span;
  }

  function apply(){
    const typeset = document.querySelector('.md-content .md-typeset');
    if (!typeset) return;

    const h1 = typeset.querySelector('h1');
    const candidates = Array.from(typeset.querySelectorAll('p'));
    let statusVal = null, updatedVal = null, typeVal = null;
    const toHide = [];

    for (const p of candidates){
      const txt = (p.textContent || '').trim();
      if (!txt) continue;
      // Avoid double-processing
      if (p.dataset.badgeProcessed === '1') continue;
      let m;
      if ((m = /^Status:\s*(.+)$/i.exec(txt))) {
        statusVal = m[1];
        toHide.push(p);
        p.dataset.badgeProcessed = '1';
      } else if ((m = /^Updated:\s*(\d{4}-\d{2}-\d{2})\b/i.exec(txt))) {
        updatedVal = m[1];
        toHide.push(p);
        p.dataset.badgeProcessed = '1';
      } else if ((m = /^Type:\s*(.+)$/i.exec(txt))) {
        typeVal = m[1];
        toHide.push(p);
        p.dataset.badgeProcessed = '1';
      }
    }

    // Fallback type inference by path when not explicitly provided
    if (!typeVal){
      const path = (location.pathname || '').toLowerCase();
      const is = (s) => path.indexOf(s) !== -1;
      if (is('/guide/quickstart')) typeVal = 'Tutorial';
      else if (is('/guide/concepts')) typeVal = 'Explanation';
      else if (is('/architecture/')) typeVal = 'Explanation';
      else if (is('/guide/')) typeVal = 'How‑to';
      else if (is('/reference/') || is('/api/') || is('/configuration') || is('/glossary') || is('/api_and_schema') || is('/gating_keys') || is('/release_notes') || is('/roadmap') || is('/interface_roadmap') || is('/backlog')) typeVal = 'Reference';
      else if (path === '/' || is('/index')) typeVal = 'Explanation';
      else if (is('/developer/') || is('/ai/')) typeVal = 'Reference';
    }

    if (h1 && (statusVal || updatedVal || typeVal)){
      const meta = document.createElement('div');
      meta.className = 'doc-meta';
      if (statusVal) meta.appendChild(makeBadge('status', statusVal));
      if (typeVal)   meta.appendChild(makeBadge('type', typeVal));
      if (updatedVal) meta.appendChild(makeBadge('updated', updatedVal));
      // Insert after H1
      if (h1.nextSibling) h1.parentNode.insertBefore(meta, h1.nextSibling);
      else h1.parentNode.appendChild(meta);
      // Hide source paragraphs (keep in DOM for anchors/links if any)
      toHide.forEach(p => { p.style.display = 'none'; });
      return;
    }

    // Fallback: style Status paragraphs inline if no H1 found
    for (const p of candidates){
      const txt = (p.textContent || '').trim();
      const m = /^Status:\s*(.+)$/i.exec(txt);
      if (!m) continue;
      if (p.dataset.badgeProcessed === '1') continue;
      const span = makeBadge('status', m[1]);
      p.textContent = '';
      p.appendChild(span);
      p.dataset.badgeProcessed = '1';
    }
  }

  if (window.document$ && typeof window.document$.subscribe === 'function'){
    window.document$.subscribe(apply);
  } else {
    document.addEventListener('DOMContentLoaded', apply);
  }
})();
