import assert from "node:assert/strict";
import test from "node:test";

import { parseApiResponse } from "../src/lib/api-response.mjs";

test("accepts bodyless successful vote and apply responses", async () => {
  for (const status of [201, 204, 205]) {
    assert.equal(await parseApiResponse(new Response(null, { status })), undefined);
  }
});

test("accepts an empty 200 response without trying to parse JSON", async () => {
  assert.equal(await parseApiResponse(new Response("", { status: 200 })), undefined);
});

test("parses JSON success responses", async () => {
  assert.deepEqual(await parseApiResponse(Response.json({ votes: 2 })), { votes: 2 });
});

test("preserves API error text for non-success responses", async () => {
  await assert.rejects(
    parseApiResponse(new Response("Already voted", { status: 409 })),
    /Already voted/,
  );
});

test("reports an empty API error with its HTTP status", async () => {
  await assert.rejects(
    parseApiResponse(new Response(null, { status: 503 })),
    /HTTP 503/,
  );
});

test("shows a readable error instead of a Cloudflare HTML error page", async () => {
  await assert.rejects(
    parseApiResponse(new Response("<!DOCTYPE html><html>Bad gateway</html>", {
      status: 502,
      headers: { "content-type": "text/html" },
    })),
    { message: "The server is temporarily unavailable (HTTP 502). Please retry in a moment." },
  );
});

test("does not hide malformed non-empty successful responses", async () => {
  await assert.rejects(
    parseApiResponse(new Response("{", { status: 200 })),
    /Invalid JSON response/,
  );
});
