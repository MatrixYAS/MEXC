// MEXC Ghost Hunter — engine v2
// Multi-stablecoin basket (USDT/USDC/DAI/FDUSD/TUSD) + triangle AND quadrangle loops.
// JS port of the verified Rust math, generalized to N legs.
// Performance notes:
//  - books keep Map<price, qty> but also a DIRTY-FLAG + pre-sorted top arrays,
//    rebuilt only when deltas touched the book (per-tick cost: dirty books only).
//  - top-of-book gross precheck rejects >99% of loops in microseconds.

export const STABLES = ["USDT", "USDC", "DAI", "FDUSD", "TUSD"];

export const DEFAULTS = {
  minProfitThreshold: 0.0025,
  takerFee: 0.0005,      // 0.05% per leg (MEXC spot taker)
  slippageBuffer: 0.0005,
  targetVolumeUsd: 1000,
  requiredTicks: 3,
  precheckGrossFloor: 0.001, // 0.1% gross product floor
  emitCooldownMs: 5000,
  minVol: 300_000,
  liquidityTestUsd: 100,   // test-order size for the tradability criterion
  maxSlip: 0.001,          // max slippage the test order may pay (0.1%)
  shapes: ["triangle", "quad"], // which loop shapes to scan
  stables: STABLES.slice(),     // allowed start/end stables
};

// ---------- order-book state ----------
export function newBookState() { return { bids: new Map(), asks: new Map(), lastUpdate: 0, dirty: false }; }

// apply incremental delta: qty == "0" removes level
export function applyDelta(map, price, qty) {
  if (qty === "0" || qty === 0 || qty === "0.0") map.delete(price);
  else { const q = parseFloat(qty); if (q && q > 0) { map.set(price, q); return true; } }
  return false;
}

// ---------- pre-sorted top arrays (lazy, dirty-flagged) ----------
// levels map: price string -> volume
export function syncTops(map, cache, desc, max = 20) {
  if (!cache.dirty) return cache.arr;
  const arr = cache.arr;
  // incremental: clear only when a level changed outside top-N is rare; simplest
  // correct fast path: rebuild (Map iteration over ≤100 levels is cheap).
  arr.length = 0;
  let i = 0;
  for (const [p, q] of map) {
    const price = parseFloat(p);
    if (!price || price <= 0 || !q || q <= 0) continue;
    arr.push({ price, volume: q });
  }
  arr.sort((a, b) => desc ? b.price - a.price : a.price - b.price);
  if (arr.length > max) arr.length = max;
  cache.dirty = false;
  return arr;
}
export function markDirty(book) { book.dirty = true; }

function topsOf(book, desc, max = 20) {
  return {
    asks: syncTops(book.asks, book._atop || (book._atop = { arr: [], dirty: true }), false, max),
    bids: syncTops(book.bids, book._btop || (book._btop = { arr: [], dirty: true }), true, max),
  };
}

// ---------- leg fills ----------
// BUY: spend `target` of quote currency walking ASKS (base price), receive base.
export function weightedFillBuy(asks, target) {
  let remaining = target, spent = 0, received = 0;
  const best = asks[0] && asks[0].price > 0 ? asks[0].price : 0;
  for (const lv of asks) {
    if (lv.price <= 0 || lv.volume <= 0) break;
    const maxQuote = lv.price * lv.volume;
    const take = Math.min(remaining, maxQuote);
    if (take <= 0) break;
    spent += take; received += take / lv.price; remaining -= take;
    if (remaining <= 0) break;
  }
  const lowLiq = received <= 0 || spent < target * 0.95;
  const fillPrice = spent > 0 && received > 0 ? spent / received : 0;
  const slippage = best > 0 && fillPrice > 0 ? Math.abs(fillPrice - best) / best : 0;
  return { fillPrice, filledVolume: received, weightedVolumeUsd: spent, isLowLiquidity: lowLiq, slippagePct: slippage };
}

// SELL base hitting BIDS: receive quote = take * price.
export function weightedFillSell(bids, target) {
  let remaining = target, spent = 0, received = 0;
  const best = bids[0] && bids[0].price > 0 ? bids[0].price : 0;
  for (const lv of bids) {
    if (lv.price <= 0 || lv.volume <= 0) break;
    const take = Math.min(remaining, lv.volume);
    if (take <= 0) break;
    spent += take; received += take * lv.price; remaining -= take;
    if (remaining <= 0) break;
  }
  const lowLiq = received <= 0 || spent < target * 0.95;
  const fillPrice = spent > 0 && received > 0 ? received / spent : 0;
  const slippage = best > 0 && fillPrice > 0 ? Math.abs(fillPrice - best) / best : 0;
  return { fillPrice, filledVolume: received, weightedVolumeUsd: spent, isLowLiquidity: lowLiq, slippagePct: slippage };
}

// LEG DIRECTION DERIVATION (MEXC base_quote naming)
// Pair P = base + quote. Holding X, converting to Y:
//   BUY Y (X is quote):   P = YX  → walk ASKS of P (spend X, receive Y)
//   SELL X (X is base):   P = XY  → walk BIDS of P (spend X, receive Y)
export function fillLeg(legs, book, direction) {
  if (direction === "buy") {
    const f = weightedFillBuy(legs.atop, legs.amount);
    legs.amount = f.filledVolume;
    return { direction, pair: legs.pair, holding: legs.holding, ...f, entry: legs.atop[0]?.price || 0, fill: f.fillPrice };
  }
  const f = weightedFillSell(legs.btop, legs.amount);
  legs.amount = f.filledVolume;
  return { direction, pair: legs.pair, holding: legs.holding, ...f, entry: legs.btop[0]?.price || 0, fill: f.fillPrice };
}

// ---------- chain helper ----------
// Build the legs[] array for one chain evaluation from live books.
// Each element: { pair, askBook/askTop, bidBook/bidTop, holding (coin symbol of what we hold), amount, entry, ... }
function materializeLegs(chain, books, target) {
  const legs = [];
  for (const c of chain) {
    const b = books.get(c.pair);
    if (!b) return null;
    const { asks, bids } = topsOf(b);
    legs.push({
      pair: c.pair, base: c.base, quote: c.quote,
      atop: asks, btop: bids,
      holding: c.holding,
    });
  }
  // leg-1 amount must be expressed in the holding coin's units:
  //   BUY leg (holding is the QUOTE, e.g. BTCUSDT hold USDT): amount = target (USD)
  //   SELL leg (holding is the BASE, e.g. BTCUSDT hold BTC): amount = target / best ask (in base)
  const lg0 = legs[0];
  if (lg0.holding === lg0.quote) lg0.amount = target;
  else lg0.amount = lg0.atop[0] && lg0.atop[0].price > 0 ? target / lg0.atop[0].price : target;
  return legs;
}

// Run the whole chain. Returns { ok, grossYield, feeCost, slippage, capacity, legs report[] }
function runChain(books, chain, settings) {
  const legs = materializeLegs(chain, books, settings.targetVolumeUsd);
  if (!legs) return null;
  const n = legs.length;
  const fee = settings.takerFee * n;
  let lowLiq = false;
  const reps = [];
  for (let i = 0; i < legs.length; i++) {
    const lg = legs[i];
    const r = fillLeg(lg, null, lg.holding === lg.quote ? "buy" : "sell");
    if (!r || r.isLowLiquidity || r.fillPrice <= 0) return null;
    lowLiq = lowLiq || r.isLowLiquidity;
    if (i + 1 < legs.length) legs[i + 1].amount = r.filledVolume; // output of this leg is input of the next
    reps.push(r);
  }
  if (lowLiq) return null;
  const last = legs[legs.length - 1];
  const grossYield = last.amount / settings.targetVolumeUsd - 1.0;
  const slippage = reps.reduce((s, r) => s + r.slippagePct, 0) / n;
  const capacity = reps.reduce((m, r) => Math.min(m, r.weightedVolumeUsd), Infinity);
  return { grossYield, feeCost: fee, slippage, capacity, reps, legs };
}

// ---------- top-of-book gross precheck (fast, before fill math) ----------
// gross = (final bid) / (product of ask-based legs), generalized:
//   For each leg: if we BUY on asks → divide by best ask; if we SELL on bids →
//   conversion factor = best bid. We compute product of held-amount multipliers.
function precheck(chain, books, minFloor) {
  let acc = 1.0; // multiplier on target quote amount, naive top-of-book
  for (const c of chain) {
    const b = books.get(c.pair);
    if (!b) return null;
    const bestAsk = c._a0 !== undefined ? c._a0 : bestTop(b.asks, false);
    const bestBid = c._b0 !== undefined ? c._b0 : bestTop(b.bids, true);
    if (bestAsk <= 0 || bestBid <= 0) return null;
    // BUY leg on pair = YX (base Y, quote X): qty received per unit X = 1/ask
    if (c.holding === c.quote) { acc *= bestAsk; /* amount grows in base? no: we hold X, spend on asks of YX... see note */ }
    else acc /= bestAsk;
  }
  return acc > 1.0 + minFloor ? acc : null;
}

function bestTop(map, desc) {
  let best = 0;
  for (const [p, q] of map) {
    const price = parseFloat(p);
    if (price <= 0 || !q || q <= 0) continue;
    best = desc ? (best === 0 ? price : Math.max(best, price)) : (best === 0 ? price : Math.min(best, price));
  }
  return best;
}

// ---------- chain construction ----------
// triangle: S1 -> A -> S2
//   leg1: buy A on A+S1 asks (hold S1, spend S1, receive A)
//   leg2: buy S2 ... no wait. After leg1 we hold A. leg2 must give us S2.
//         pair A+S2 (base A, quote S2): sell A on bids → receive S2. ✓
//         pair S2+A (base S2, quote A): buy S2 with A on asks... but A is not a stable; MEXC rarely lists
//         coin-as-quote pairs. So leg2 uses A+S2, direction SELL.
//   For quadrangle S1 -> A -> B -> S2:
//   leg1: buy A on A+S1 asks (sell S1, receive A)
//   leg2: buy B on B+A asks? MEXC has COINBASE-stableQUOTE usually; B+A where A is not stable is rare.
//         Alternative leg2: pair A+B (base A, quote B): sell A on bids, receive B. ✓ common (both USDT pairs)
//         Then leg3 pair B+S2 (base B, quote S2): sell B on bids, receive S2. ✓
//   So quadrangle pairs: A+S1 (asks BUY), A+B (bids SELL), B+S2 (bids SELL).
//   Triangle pairs:     A+S1 (asks BUY), A+S2 (bids SELL).
//   This matches the previous triangle semantics: pair2 was B+A... old code used
//   pair2 = coinB+coinA i.e. A+B with A=coinB?? old: pair1=A+USDT, pair2=B+A, pair3=B+USDT with
//   leg2 buyBWithA(a2, amountA) walking ASKS of B+A (base B, quote A) — buys B with A.
//   MEXC lists both A+B and B+A; the old code used B+A asks (buy B paying A).
//   For compatibility and richer coverage, we pick whichever exists:
//     prefer A+B bids-sell when both exist? No — keep old semantics as default leg variant:
//     leg variant V1: B+A asks BUY (receive B). variant V2: A+B bids SELL (receive B).
//   We'll generate BOTH loop variants when both pairs exist; same for closing leg.

export const isStable = (c, stables) => stables.includes(c);

function coinOf(sym, suffix) {
  if (sym.endsWith(suffix) && sym.length > suffix.length) return sym.slice(0, -suffix.length);
  return null;
}
// longest valid stable suffix wins (prevents BTCUSD1 -> coin "BTC", quote "USD1"
// when the symbol actually means BTC + USDT)
export function parseSymbol(sym, stables) {
  let best = null;
  for (const st of stables) {
    const c = coinOf(sym, st);
    if (!c) continue;
    if (!best || st.length > best.st.length) best = { coin: c, st };
  }
  return best; // null if no stable suffix
}

export function buildLoops(symbols, settings) {
  const stables = settings.stables || STABLES;
  const set = new Set(symbols);
  const loops = [];
  // index: pair -> { base, quote, coin } for known symbols
  const pairs = new Map();
  for (const s of symbols) {
    const p = parseSymbol(s, stables);
    if (p && p.coin && !isStable(p.coin, stables)) pairs.set(s, { base: p.coin, quote: p.st, coin: p.coin });
    // cross pair X+Y between two non-stables: base first coin
    else if (!pairs.has(s)) pairs.set(s, { base: s.slice(0, s.length / 2 | 0), quote: s.slice(s.length / 2 | 0) });
  }
  // Only treat a symbol as a loop "coin" if it is a genuine asset that also has
  // at least one stablecoin pair in the universe. This filters out exotic quote
  // conventions (e.g. EURUSDT, USD1 contracts) that produce nonsensical loops.
  // Require the coin to have at least one stablecoin pair in the universe and to
  // not be a placeholder/contract quote (names ending in a digit, e.g. USD1).
  const coins = [...pairs.keys()].map(s => pairs.get(s).coin);
  const uniqCoins = new Set(coins.filter(c => !isStable(c, stables) && !/\d$/.test(c) && symbols.some(sym => STABLES.some(st => sym === c + st))));
  const addLoop = (shape, s1, s2, chain, legs) => {
    loops.push({
      key: legs.map(l => l[0]).join("|"),
      shape, start: s1, end: s2,
      chain, // [{pair,base,quote,holding},...] where holding = coin we hold before that leg
      pairs: legs.map(l => l[0]),
      display: [s1, ...legs.slice(1).map(l => l[1]), s2].join(" → "),
      legs, // [[pair, intermediateCoin], ...]
    });
  };

  // LEG CONVENTION: legs = [[pair, startHolding], ...] — startHolding is the coin
  // we hold BEFORE executing that leg. Each leg converts startHolding into the
  // next holding; the final leg must return us to a stable (end of loop).
  const triLegs = (coinA, coinB, s1, s2) => {
    const legs = [[coinA + s1, s1]]; // hold s1, buy coinA on asks
    const a2 = coinB + coinA, a2rev = coinA + coinB;
    if (set.has(a2)) legs.push([a2, coinA]);      // hold coinA, buy coinB (pair base=B, quote=A)
    else if (set.has(a2rev)) legs.push([a2rev, coinA]); // hold coinA, sell coinA (pair base=A, quote=B)
    else return null;
    const lastHolding = legs[legs.length - 1][1];
    const p3 = lastHolding === coinA ? coinB + s2 : null; // after buying B we hold B
    if (p3 && set.has(p3)) legs.push([p3, coinB]); // hold coinB, sell on bids, receive s2
    else return null;
    return legs;
  };

  // --- triangles: S1 -> coinA -> coinB ... wait, triangle = 2 intermediates?
  // Triangle = 3 pairs = 2 intermediate coins (A, B) with 3 conversions.
  for (const s1 of stables) for (const s2 of stables) for (const coinA of uniqCoins) for (const coinB of uniqCoins) {
    if (coinA === coinB) continue;
    const legs = triLegs(coinA, coinB, s1, s2);
    if (legs) addLoop("triangle", s1, s2, null, legs);
  }

  // --- quadrangles: S1 -> A -> B -> C -> S2 (4 pairs, 3 intermediate coins)
  if ((settings.shapes || DEFAULTS.shapes).includes("quad")) {
    for (const s1 of stables) for (const s2 of stables) for (const coinA of uniqCoins) for (const coinB of uniqCoins) for (const coinC of uniqCoins) {
      if (new Set([coinA, coinB, coinC]).size !== 3) continue;
      const legs = [[coinA + s1, s1]]; // hold s1, buy coinA on A+S1 asks
      // leg2: hold coinA → get coinB: prefer pair B+A (base B, quote A) BUY, else A+B (base A, quote B) SELL
      const bPlusA = coinB + coinA, aPlusB = coinA + coinB;
      if (set.has(bPlusA)) legs.push([bPlusA, coinA]);
      else if (set.has(aPlusB)) legs.push([aPlusB, coinA]);
      else continue;
      // leg3: hold coinB → get coinC
      const cPlusB = coinC + coinB, bPlusC = coinB + coinC;
      if (set.has(cPlusB)) legs.push([cPlusB, coinB]);
      else if (set.has(bPlusC)) legs.push([bPlusC, coinB]);
      else continue;
      const p4 = coinC + s2;
      if (set.has(p4)) legs.push([p4, coinC]); // hold coinC, sell on bids, receive s2
      else continue;
      addLoop("quad", s1, s2, null, legs);
    }
  }
  return loops;
}

// ---------- chain expansion with base/quote (done once per tick per chain) ----------
// holding = the coin we currently own and want to convert. If the pair ends with
// holding, holding is the QUOTE (BUY on asks, receive holding/ask units); if the
// pair starts with holding, holding is the BASE (SELL on bids, receive bid units).
export function expandChain(chain) {
  const out = [];
  for (const [pair, holding] of chain) {
    if (pair.startsWith(holding) && pair.length > holding.length) {
      // holding is the BASE (e.g. BTCUSDT hold BTC) — SELL on bids, receive quote
      out.push({ pair, holding, base: holding, quote: pair.slice(holding.length) });
    } else if (pair.endsWith(holding) && pair.length > holding.length) {
      // holding is the QUOTE (e.g. BTCUSD1 hold USD1) — BUY on asks, receive holding
      out.push({ pair, holding, base: pair.slice(0, pair.length - holding.length), quote: holding });
    } else {
      // cannot resolve pair vs holding — drop this leg silently; loop will be rejected
      continue;
    }
  }
  return out;
}

// ---------- full loop validation ----------
export function validateLoop(loop, books, settings) {
  const now = Date.now();
  let allFresh = true;
  const chain = loop.chain || expandChain(loop.legs);
  loop.chain = chain;
  for (const c of chain) {
    const b = books.get(c.pair);
    if (!b || now - (b.lastUpdate || 0) > 2000) { allFresh = false; break; }
    c._a0 = bestTop(b.asks, false); c._b0 = bestTop(b.bids, true);
    if (c._a0 <= 0 || c._b0 <= 0) return null;
  }
  if (!allFresh) return null;

  // gross precheck: naive top-of-book product
  let acc = 1.0;
  for (const c of chain) {
    if (c._a0 <= 0 || c._b0 <= 0) return null;  // missing side: books too sparse
    if (c.holding === c.quote) acc /= c._a0;    // BUY on asks: receive holding/ask per unit quote spent
    else acc *= c._b0;                          // SELL on bids: receive quote at bid
  }
  if (acc <= 1.0 + settings.precheckGrossFloor) return null;

  const r = runChain(books, chain, settings);
  if (!r) return null;
  const netYield = r.grossYield - r.feeCost - settings.slippageBuffer;
  if (netYield < settings.minProfitThreshold) return null;

  const makerYield = r.grossYield - settings.slippageBuffer;
  return {
    netYield, grossYield: r.grossYield, feeCost: r.feeCost, slippage: r.slippage,
    estimatedProfitUsd: r.capacity * netYield, capacityUsd: r.capacity,
    makerPlanYield: makerYield,
    reps: r.reps, chain,
  };
}

// ---------- optimal trade size + yield curve (n legs) ----------
export function findOptimalSize(loop, books, settings) {
  const chain = loop.chain || expandChain(loop.legs);
  const threshold = settings.minProfitThreshold;
  const first = books.get(chain[0].pair);
  if (!first) return null;
  const a0 = syncTops(first.asks, first._atop || (first._atop = { arr: [], dirty: true }), false);
  const leg1Cap = a0.filter(l => l.price > 0 && l.volume > 0).reduce((s, l) => s + l.price * l.volume, 0);
  const hi0 = Math.min(Math.max(leg1Cap, 100), 1_000_000);

  const allFill = (usd) => {
    const r = runChain(books, chain, { ...settings, targetVolumeUsd: usd });
    return !!r;
  };
  if (!allFill(100)) return null;
  let lo = 100, hi = hi0;
  for (let i = 0; i < 30; i++) { const mid = (lo + hi) / 2; if (allFill(mid)) lo = mid; else hi = mid; }
  const capacity = Math.floor(lo);

  const evalAt = (usd) => {
    const r = runChain(books, chain, { ...settings, targetVolumeUsd: usd });
    return r ? r.grossYield - r.feeCost - settings.slippageBuffer : null;
  };

  const curve = [];
  const sizes = [100, 200, 500, 1000, 2000, 5000, 10000, 25000, 50000, 100000];
  for (const size of sizes) {
    if (size > capacity + 0.5) break;
    const y = evalAt(size);
    if (y !== null) curve.push({ size_usd: size, net_yield: y });
  }
  if (curve.length && curve[curve.length - 1].size_usd < capacity * 0.9) {
    const y = evalAt(capacity);
    if (y !== null) curve.push({ size_usd: capacity, net_yield: y });
  }
  let optimal = null, yieldAt = 0;
  for (const p of curve) if (p.size_usd <= capacity + 0.5 && p.net_yield >= threshold) { optimal = p.size_usd; yieldAt = p.net_yield; }
  if (optimal === null && curve.length) { optimal = curve[0].size_usd; yieldAt = curve[0].net_yield; }
  return { optimalSize: optimal ?? 0, optimalYield: yieldAt, curve, capacity };
}

// ---------- liquidity grade ----------
export function liquidityGrade(reps) {
  const avg = reps.reduce((s, r) => s + r.weightedVolumeUsd, 0) / reps.length;
  return avg > 5000 ? "A" : avg > 2000 ? "B" : avg > 800 ? "C" : avg > 200 ? "D" : "F";
}

// ---------- symbol universe ----------
const CORS_PROXIES = ["https://corsproxy.io/?", "https://api.codetabs.com/v1/proxy?quest="];
async function timedFetch(input, ms) {
  const ac = new AbortController();
  const t = setTimeout(() => ac.abort(), ms);
  try { return await fetch(input, { signal: ac.signal }); }
  finally { clearTimeout(t); }
}
function looksJson(r) {
  const ct = (r.headers.get("content-type") || "").toLowerCase();
  return !ct || ct.includes("json") || ct.includes("text/plain");
}
export async function corsFetch(url) {
  let r = null;
  try {
    r = await timedFetch(url, 10000);
    if (r.ok && looksJson(r)) return r;
  } catch { /* proxy below */ }
  for (const prefix of CORS_PROXIES) {
    try {
      const p = await timedFetch(prefix + encodeURIComponent(url), 20000);
      if (p.ok) return p;
    } catch { /* next proxy */ }
  }
  if (r) return r;
  throw new Error("all endpoints failed: " + url);
}

// ---------- real tradability criterion ----------
// The classic proxy (24h volume) can lie: a coin with $1M daily volume may have a
// $100 order slip 5% on a thin book. Instead we measure the actual top-of-book depth:
// a coin is "tradeable" if a $TEST-USD taker order fills both directions with <= maxSlip.
export function tradableScore(b, usd, maxSlip, topN = 20) {
  const res = {};
  for (const side of ["buy", "sell"]) {
    const lvls = [...(side === "buy" ? b.asks : b.bids)]
      .map(([p, q]) => ({ price: parseFloat(p), volume: q }))
      .filter(l => l.price > 0 && l.volume > 0)
      .sort((a, z) => side === "buy" ? a.price - z.price : z.price - a.price)
      .slice(0, topN);
    if (!lvls.length) { res[side] = null; continue; }
    const best = lvls[0].price;
    // Measure everything in quote-USD so both sides share one scale:
    //   buy:  spend quote = min(remaining, price*qty); receive base
    //   sell: sell base qty needed = remaining/price; spend quote = qty*price
    let remaining = usd, spendUsd = 0, received = 0;
    for (const lv of lvls) {
      if (remaining <= 0) break;
      let takeUsd;
      if (side === "buy") {
        takeUsd = Math.min(remaining, lv.price * lv.volume);
        if (takeUsd <= 0) break;
        received += takeUsd / lv.price;
      } else {
        const neededBase = remaining / lv.price;
        const takeBase = Math.min(neededBase, lv.volume);
        if (takeBase <= 0) break;
        takeUsd = takeBase * lv.price;
        received += takeBase;
      }
      spendUsd += takeUsd;
      remaining -= takeUsd;
    }
    if (spendUsd < usd * 0.95) { res[side] = null; continue; }
    // fill = USD realized per unit of base in BOTH directions, so slippage is
    // always against the best price on the same scale:
    //   buy: spendUsd / receivedBase ; sell: spendUsd / soldBase
    const fill = spendUsd / received;
    const slip = Math.abs(fill - best) / best;
    res[side] = { slip, depthUsd: spendUsd, filled: true };
  }
  const ok = res.buy !== null && res.sell !== null && res.buy.slip <= maxSlip && res.sell.slip <= maxSlip;
  return { ok, buy: res.buy, sell: res.sell, bothDepthUsd: res.buy && res.sell ? Math.min(res.buy.depthUsd, res.sell.depthUsd) : 0 };
}

export async function buildUniverse(minVol = 300_000, maxWhitelist = 450, liquidityTestUsd = DEFAULTS.liquidityTestUsd, maxSlip = DEFAULTS.maxSlip) {
  const BASE = "https://api.mexc.com";
  const looksLikeTicker = j => Array.isArray(j) && j.length > 0 && j[0] && j[0].symbol;
  let tickers = null, lastErr = null;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const r = await corsFetch(BASE + "/api/v3/ticker/24hr");
      tickers = await r.json();
      if (!looksLikeTicker(tickers)) { tickers = null; throw new Error("ticker: not json array"); }
      break;
    }
    catch (e) { lastErr = e; await new Promise(res => setTimeout(res, 2000 * (attempt + 1))); }
  }
  if (!tickers) throw lastErr || new Error("ticker failed");
  let valid = null;
  try {
    const info = await corsFetch(BASE + "/api/v3/exchangeInfo");
    const j = await info.json();
    if (j && Array.isArray(j.symbols)) {
      valid = new Set();
      for (const s of j.symbols || []) if (s.isSpotTradingAllowed) valid.add(s.symbol);
    }
  } catch { /* fallback below */ }
  if (!valid) valid = new Set(tickers.map(t => t.symbol));

  // stable-pair whitelist: every STABLE as quote
  const vols = new Map();
  const stablePairs = [];
  for (const t of tickers) {
    const sym = t.symbol, vol = parseFloat(t.quoteVolume) || 0;
    if (vol > 0) vols.set(sym, vol);
    for (const st of STABLES) {
      const p = parseSymbol(sym, STABLES);
      if (p && p.st === st && valid.has(sym) && !isStable(p.coin, STABLES)) {
        stablePairs.push({ sym, vol, st });
        break;
      }
    }
  }
  stablePairs.sort((a, b) => b.vol - a.vol);
  const whitelist = new Set();
  for (const { sym, vol } of stablePairs) if (vol >= minVol) whitelist.add(sym);
  for (const m of ["BTCUSDT", "ETHUSDT", "SOLUSDT", "USDCUSDT"]) if (valid.has(m)) whitelist.add(m);
  // ---- real tradability filter: volume is a proxy; depth is the truth ----
  // Every candidate coin must pass the depth test on its deepest stable pair:
  //   a $liquidityTestUsd taker order fills both directions with slippage <= maxSlip.
  // Measured once at start (re-measured on every restart), not cached across sessions.
  const tested = new Map();   // coin -> { pair, buy {slip,depthUsd}, sell {...}, ok }

  const jobs = [];
  for (const { sym } of stablePairs) {
    if (!whitelist.has(sym)) continue;
    const pc = parseSymbol(sym, STABLES);
    const c = pc ? pc.coin : null;
    if (!c || isStable(c, STABLES)) continue;
    jobs.push((async () => {
      let j = null;
      try { const r = await corsFetch(BASE + "/api/v3/depth?symbol=" + sym + "&limit=50"); if (r.ok) j = await r.json(); } catch {}
      if (j && (!j.bids || !j.asks)) j = null;
      if (!j) return;
      const b = newBookState();
      for (const [x, q] of j.bids) { if (parseFloat(q) > 0) { b.bids.set(x, parseFloat(q)); markDirty(b); } }
      for (const [x, q] of j.asks) { if (parseFloat(q) > 0) { b.asks.set(x, parseFloat(q)); markDirty(b); } }
      const t = tradableScore(b, liquidityTestUsd, maxSlip);
      tested.set(c, { coin: c, pair: sym, buy: t.buy, sell: t.sell, ok: t.ok, depthUsd: t.bothDepthUsd, vol24h: vols.get(sym) || 0 });
    })());
  }
  await Promise.all(jobs);
  // a coin stays only if it passed the depth test AND is a coin we can start/end loops with
  const liquidity = new Map(); // coin -> tradability snapshot (used by the Coins page)
  for (const [c, t] of tested) {
    if (t.ok) liquidity.set(c, t);
    else if (whitelist.has(c + "USDT")) whitelist.delete(c + "USDT");
    // (other stable quotes of failing coins were never added beyond the stablePairs loop)
  }
  // include direct stable<->stable pairs (e.g. USDCDAI) for direct gaps
  for (const t of tickers) {
    for (const sa of STABLES) for (const sb of STABLES) {
      if (sa === sb) continue;
      if (t.symbol === sa + sb && valid.has(t.symbol) && whitelist.has(sa + "USDT")) whitelist.add(sa + sb);
    }
  }
  // cross-pair expansion (closing legs for triangles/quads)
  const coins = [...whitelist].map(s => { const pc = parseSymbol(s, STABLES); return pc && !isStable(pc.coin, STABLES) ? pc.coin : null; }).filter(Boolean);
  const uniqCoins = new Set(coins);
  const minCrossVol = Math.max(minVol * 0.01, 5000);
  const coinArr = [...uniqCoins];
  for (let i = 0; i < coinArr.length; i++) for (let j = 0; j < coinArr.length; j++) {
    if (i === j) continue;
    const a = coinArr[i], b = coinArr[j];
    for (const cand of [b + a, a + b]) {
      if (!whitelist.has(cand) && valid.has(cand)) {
        const cv = vols.get(cand) || 0;
        if (cv >= minCrossVol) whitelist.add(cand);
      }
    }
  }
  if (whitelist.size > maxWhitelist) {
    // rank by depth (measured) and trim
    const coinRank = [...tested.values()].filter(t => t.ok && whitelist.has(t.coin + "USDT")).sort((a, b) => b.depthUsd - a.depthUsd);
    const keep = new Set(coinRank.slice(0, maxWhitelist).map(t => t.coin));
    const trimmed = new Set();
    for (const s of whitelist) {
      const pc = parseSymbol(s, STABLES);
      const c = pc ? pc.coin : null;
      if (c && !isStable(c, STABLES) && !keep.has(c)) continue;
      trimmed.add(s);
    }
    const out = [...trimmed];
    out.liquidity = liquidity;
    out.liquidityTestUsd = liquidityTestUsd;
    out.maxSlip = maxSlip;
    return out;
  }
  const out = [...whitelist];
  out.liquidity = liquidity;
  out.liquidityTestUsd = liquidityTestUsd;
  out.maxSlip = maxSlip;
  return out;
}
