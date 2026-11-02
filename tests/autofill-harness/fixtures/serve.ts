// Serves the fixture pages over HTTPS (port 8443) and HTTP (port 8081) for
// every *.test host name the browser maps to 127.0.0.1. HTTPS matters: the
// engine warns before filling on http pages (D33). The certificate is a
// throwaway self-signed one made with openssl at start.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import https from "node:https";
import os from "node:os";
import path from "node:path";

const dir = import.meta.dirname;
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pwm-fixtures-"));
const key = path.join(tmp, "key.pem");
const cert = path.join(tmp, "cert.pem");
execFileSync("openssl", [
  "req",
  "-x509",
  "-newkey",
  "rsa:2048",
  "-nodes",
  "-days",
  "1",
  "-subj",
  "/CN=fixtures.test",
  "-keyout",
  key,
  "-out",
  cert,
]);

function handler(req: http.IncomingMessage, res: http.ServerResponse): void {
  const url = new URL(req.url ?? "/", "https://fixtures.test");
  if (url.pathname === "/health") {
    res.writeHead(200).end("ok");
    return;
  }
  // For the runner's own tests (D39 §5): a server error and a page that
  // never answers in time.
  if (url.pathname === "/status-503") {
    res
      .writeHead(503, { "content-type": "text/html" })
      .end("<h1>Unavailable</h1>");
    return;
  }
  if (url.pathname === "/slow") {
    setTimeout(() => res.writeHead(200).end("late"), 30_000);
    return;
  }
  const name = path.basename(url.pathname);
  const file = path.join(dir, name);
  if (!name.endsWith(".html") || !fs.existsSync(file)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  res.end(fs.readFileSync(file));
}

https
  .createServer(
    { key: fs.readFileSync(key), cert: fs.readFileSync(cert) },
    handler,
  )
  .listen(8443, "127.0.0.1");
http.createServer(handler).listen(8081, "127.0.0.1");
console.log("fixtures on https://*.test:8443 and http://*.test:8081");
