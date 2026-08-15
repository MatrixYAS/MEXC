// JS port of backend/src/network/pb.rs
// Decodes MEXC `spot@public.aggre.depth.v3.api.pb` WebSocket frames.
// Wire layout (verified by live captures):
//   field 1 (string) = channel, field 3 (string) = symbol, field 313 (bytes) = depth payload
//   payload inner: field 1 (bytes) = asks levels, field 2 (bytes) = bids levels
//   level: field 1 (string) = price, field 2 (string) = quantity
// qty "0" means the level was removed.

const DBG = false;

export function decodeDepthFrame(raw) {
  let pos = 0;
  const out = { channel: "", symbol: "", bids: [], asks: [] };
  let payloadSeen = false;

  const n = raw.byteLength !== undefined ? raw.byteLength : raw.length;
  // Node ws hands back Buffers; browser gives offset-0 ArrayBuffers. Copy either way.
  const isAB = raw instanceof ArrayBuffer;
  const srcView = raw instanceof Uint8Array ? raw : new Uint8Array(isAB ? raw : raw.buffer, isAB ? 0 : (raw.byteOffset || 0), raw.byteLength || 0);
  const buf = new Uint8Array(srcView);

  while (pos < n) {
    const [tag, tr] = readVarint(buf, pos);
    if (DBG) console.log('outer pos', pos, 'tag', tag, 'tr', tr);
    if (tag === null) return null;
    pos += tr;
    const wire = tag & 0x07;
    const field = tag >>> 3;

    if (wire === 0) {
      const [, vr] = readVarint(buf, pos);
      if (vr === 0) return null;
      pos += vr;
    } else if (wire === 2) {
      const [len, lr] = readVarint(buf, pos);
      if (len === null || lr === 0) return null;
      pos += lr;
      const end = pos + len;
      if (end > n) return null;
      if (field === 1 && !out.channel) {
        out.channel = utf8(buf, pos, end);
      } else if (field === 3) {
        out.symbol = utf8(buf, pos, end);
      } else if (field === 313) {
        if (DBG) console.log('payload at', pos, 'len', len);
        const inner = decodeDepthInner(buf, pos, end);
        if (DBG && !inner) console.log('inner returned null');
        if (inner) {
          if (inner.bids.length || inner.asks.length) {
            out.bids.push(...inner.bids);
            out.asks.push(...inner.asks);
            payloadSeen = true;
          }
        }
      }
      pos = end;
    } else if (wire === 1) {
      pos += 8;
    } else if (wire === 5) {
      pos += 4;
    } else {
      return null;
    }
  }

  if (out.channel.includes("aggre.depth") || payloadSeen) return out;
  return null;
}

function decodeDepthInner(buf, start, end) {
  let pos = start;
  const bids = [], asks = [];
  while (pos < end) {
    const [tag, tr] = readVarint(buf, pos);
    if (tag === null) return null;
    pos += tr;
    const wire = tag & 0x07;
    const field = tag >>> 3;
    if (DBG) console.log('  inner pos', pos - tr, 'field', field, 'wire', wire);
    if (wire === 2) {
      const [len, lr] = readVarint(buf, pos);
      if (len === null || lr === 0) return null;
      pos += lr;
      const lvlEnd = pos + len;
      if (lvlEnd > end) return null;
      if (field === 1 || field === 2) {
        const lvl = decodeLevel(buf, pos, lvlEnd);
        if (DBG) console.log('    lvl field', field, '->', lvl);
        if (!lvl) return null;
        if (field === 1) asks.push(lvl); // verified: field 1 = asks side
        else bids.push(lvl); // field 2 = bids side
      } // other repeated fields (channel/symbol echoed inside payload) are skipped
      pos = lvlEnd;
    } else if (wire === 0) {
      const [, vr] = readVarint(buf, pos);
      if (vr === 0) return null;
      pos += vr;
    } else if (wire === 1) { pos += 8; } else if (wire === 5) { pos += 4; } else { return null; }
  }
  return { bids, asks };
}

function decodeLevel(buf, start, end) {
  let pos = start;
  let price = "", qty = "";
  while (pos < end) {
    const [tag, tr] = readVarint(buf, pos);
    if (tag === null) return null;
    pos += tr;
    const wire = tag & 0x07;
    const field = tag >>> 3;
    if (wire === 2) {
      const [len, lr] = readVarint(buf, pos);
      if (len === null || lr === 0) return null;
      pos += lr;
      const s = utf8(buf, pos, pos + len);
      if (field === 1) price = s;
      else if (field === 2) qty = s;
      else if (DBG) console.log('    LVL unknown field', field, 'wire', wire, 'len', len);
      pos += len;
    } else if (wire === 0) {
      const [, vr] = readVarint(buf, pos);
      if (vr === 0) return null;
      pos += vr;
    } else if (wire === 1) { pos += 8; } else if (wire === 5) { pos += 4; } else { if (DBG) console.log('    LVL weird wire', wire); return null; }
  }
  return [price, qty];
}

function readVarint(buf, pos) {
  let value = 0, shift = 0, p = pos;
  for (let i = 0; i < 10; i++) {
    if (p >= buf.length) return [null, 0];
    const b = buf[p++];
    value |= (b & 0x7f) * Math.pow(2, shift); // safe: tags are small
    shift += 7;
    if ((b & 0x80) === 0) return [value, p - pos];
  }
  return [null, 0];
}

function utf8(buf, start, end) {
  try { return new TextDecoder().decode(buf.subarray(start, end)); }
  catch { return ""; }
}
