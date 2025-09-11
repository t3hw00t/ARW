// Lightweight helpers shared by launcher pages
window.ARW = {
  invoke(cmd, args) {
    return window.__TAURI__.invoke(cmd, args)
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
  }
}

