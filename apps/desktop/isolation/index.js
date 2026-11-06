// Tauri's isolation pattern (D3 §3): every message from the window passes
// here first. Only the app's one command goes through; anything else is
// dropped, whatever a script in the page tried to send.
window.__TAURI_ISOLATION_HOOK__ = (payload) => {
  if (payload && payload.cmd === "request") return payload;
  return undefined;
};
