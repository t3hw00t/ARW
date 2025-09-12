// Lightweight helpers shared by launcher pages
window.ARW = {
  invoke(cmd, args) {
    return window.__TAURI__.invoke(cmd, args)
  },
  toast(msg) {
    if (!this._toastWrap) {
      const wrap = document.createElement('div');
      wrap.className = 'toast-wrap';
      document.body.appendChild(wrap);
      this._toastWrap = wrap;
    }
    const d = document.createElement('div');
    d.className = 'toast'; d.textContent = msg;
    this._toastWrap.appendChild(d);
    setTimeout(()=>{ try{ this._toastWrap.removeChild(d); }catch(e){} }, 2500);
  },
  async getPrefs(ns = 'launcher') {
    try { return await this.invoke('get_prefs', { namespace: ns }) } catch { return {} }
  },
  async setPrefs(ns, value) {
    return this.invoke('set_prefs', { namespace: ns, value })
  },
  base(port) {
    const p = Number.isFinite(port) && port > 0 ? port : 8090
    return `http://127.0.0.1:${p}`
  },
  getPortFromInput(id) {
    const v = parseInt(document.getElementById(id)?.value, 10)
    return Number.isFinite(v) && v > 0 ? v : null
  },
  async applyPortFromPrefs(id, ns = 'launcher') {
    const v = await this.getPrefs(ns)
    if (v && v.port && document.getElementById(id)) document.getElementById(id).value = v.port
  },
  quantReplace(url, q) {
    try {
      if (!url || !/\.gguf$/i.test(url)) return url
      // Replace existing quant token like Q4_K_M, Q5_K_S, Q8_0 etc., else insert before .gguf
      const m = url.match(/(.*?)(Q\d[^/]*?)?(\.gguf)$/i)
      if (!m) return url
      const prefix = m[1]
      const has = !!m[2]
      const ext = m[3]
      if (has) return prefix + q + ext
      // insert with hyphen if the filename part doesn't already end with '-'
      return url.replace(/\.gguf$/i, (prefix.endsWith('-') ? '' : '-') + q + '.gguf')
    } catch { return url }
  }
}
