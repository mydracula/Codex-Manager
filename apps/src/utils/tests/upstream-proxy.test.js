import test from "node:test";
import assert from "node:assert/strict";

import { normalizeUpstreamProxyUrl } from "../upstream-proxy.js";

test("normalizeUpstreamProxyUrl preserves http proxies", () => {
  assert.equal(normalizeUpstreamProxyUrl(" http://127.0.0.1:8080 "), "http://127.0.0.1:8080");
});

test("normalizeUpstreamProxyUrl rewrites socks schemes to socks5h", () => {
  assert.equal(normalizeUpstreamProxyUrl("socks5://127.0.0.1:7890"), "socks5h://127.0.0.1:7890");
  assert.equal(normalizeUpstreamProxyUrl("socks://127.0.0.1:7891"), "socks5h://127.0.0.1:7891");
});

test("normalizeUpstreamProxyUrl strips duplicated http scheme before socks", () => {
  assert.equal(
    normalizeUpstreamProxyUrl("https://socks5://127.0.0.1:7892"),
    "socks5h://127.0.0.1:7892",
  );
  assert.equal(
    normalizeUpstreamProxyUrl("http://socks://127.0.0.1:7893"),
    "socks5h://127.0.0.1:7893",
  );
});
