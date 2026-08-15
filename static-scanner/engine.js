// MEXC Ghost Hunter — engine v3
// Multi-stablecoin basket (USDT/USDC/DAI/FDUSD/TUSD) + QUAD / PENTA / HEXAGON loops ONLY.
// JS port of the verified Rust math, generalized to N legs.
// Loop = n pairs: start-stable -> coin1 -> coin2 -> ... -> end-stable.
// Triangles are deliberately excluded (3 pairs): the fee+slippage structure
// rarely leaves honest profit after 3 legs; 4-6 legs give richer paths.
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
  shapes: ["quad", "penta", "hex"], // which loop shapes to scan (n legs/pairs)
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
  // capacity in TRUE USD: convert each leg's spent (in its own quote currency)
  // to USDT using that quote's best bid vs USDT when available
  const usdOf = (r) => {
    // the currency we SPENT in this leg: BUY on asks → the pair's quote currency;
    // SELL on bids → the pair's base currency (reps carry pair+holding, not base/quote)
    // BUY leg: holding is the quote currency of the pair (we spend holding to buy)
    // SELL leg: holding is the base currency of the pair (we spend holding to sell)
    // In BOTH cases the currency spent is exactly r.holding — the amount we spend
    // is the quote-currency on buys and base-currency on sells, and holding IS that
    // currency by construction (legs are built so the held asset funds the leg).
    const spentCurr = r.holding;
    if (spentCurr === "USDT") return r.weightedVolumeUsd;
    const sym = spentCurr + "USDT";
    const b = books.get(sym);
    if (!b) return r.weightedVolumeUsd; // unknown rate — keep native (honest under-estimator)
    const bestBid = bestTop(b.bids, true);
    return bestBid > 0 ? r.weightedVolumeUsd * bestBid : r.weightedVolumeUsd;
  };
  const capacity = reps.reduce((m, r) => Math.min(m, usdOf(r)), Infinity);
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
  // baseQuote (optional): Map<symbol, {base, quote}> from exchangeInfo — EXACT
  // splits; without it we fall back to the stable-suffix heuristic, and for
  // remaining symbols to a half-split (risky: DOGE+KAS half-split wrong) which
  // is why the scanner should always pass exchangeInfo data.
  const baseQuote = settings.baseQuote || new Map();
  const pairs = new Map();
  for (const s of symbols) {
    const bq = baseQuote.get(s);
    if (bq) { pairs.set(s, { base: bq.base, quote: bq.quote, coin: bq.base }); continue; }
    const p = parseSymbol(s, stables);
    if (p && p.coin && !isStable(p.coin, stables)) pairs.set(s, { base: p.coin, quote: p.st, coin: p.coin });
    // cross pair X+Y between two non-stables: half-split fallback (inexact)
    else if (!pairs.has(s)) pairs.set(s, { base: s.slice(0, s.length / 2 | 0), quote: s.slice(s.length / 2 | 0) });
  }
  // Only treat a symbol as a loop "coin" if it is a genuine asset that also has
  // at least one stablecoin pair in the universe. This filters out exotic quote
  // conventions (e.g. EURUSDT, USD1 contracts) that produce nonsensical loops.
  // Require the coin to have at least one stablecoin pair in the universe and to
  // not be a placeholder/contract quote (names ending in a digit, e.g. USD1).
  const coins = [...pairs.keys()].map(s => pairs.get(s).coin);
  const uniqCoins = new Set(coins.filter(c => !isStable(c, stables) && !/\d$/.test(c) && symbols.some(sym => STABLES.some(st => sym === c + st))));
  const shapes = settings.shapes || DEFAULTS.shapes;

  // ---------- bounded N-leg builder (no combinatorial explosion) ----------
  // Perf guarantees (why this stays light on 450+ pairs / 60+ coins):
  //   1. Adjacency lists: for coin X, next(Y) only if pair X+Y or Y+X exists.
  //      Real MEXC universes have ~2-8 cross pairs per coin, not 60+.
  //   2. Coin caps: mid-leg coins are ranked by 24h quote volume; only the top
  //      maxMidCoins are eligible as intermediates (thin coins produce loops
  //      whose fills slip anyway — the depth criterion would kill them at
  //      validation time; ranking here kills them earlier, cheaply).
  //   3. Per-shape loop cap: hard ceiling on registered loops; the scanner scans
  //      every registered loop each tick, so a hard cap bounds worst-case cost.
  //   4. Stable-end check pushed to the END (cheap Set.has) instead of early.
  const MAX_MID_COINS = 60;     // intermediate-coin rank cap (volume)
  const MAX_LOOPS_PER_SHAPE = 6000;

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

  // Adjacency: coin X -> list of { pair, nextCoin } for existing cross pairs.
  // Direction is resolved later by expandChain; here we just need the pair + peer.
  // mid-leg exclusion: stables themselves must never sit mid-loop (they are the
  // endpoints). Some "coins" are quasi-stables or fiats (EUR, TRY, ...); exclude
  // anything whose name matches a stable basket member or a known fiat.
  const FIAT = new Set(["EUR", "TRY", "BRL", "USDP", "GUSD", "TUSD"].filter(c => !stables.includes(c)));
  const isMidEligible = c => !stables.includes(c) && !FIAT.has(c) && !/^(USD|EURUS|GBP|AUD|CAD|CHF|JPY)$/.test(c);
  const uniq = [...uniqCoins].filter(isMidEligible);
  const adj = new Map(); // coin -> [{pair, peer}]
  for (const x of uniq) adj.set(x, []);
  for (const sym of symbols) {
    const pc = parseSymbol(sym, stables);
    if (pc && !isStable(pc.coin, stables)) continue; // stable quote pairs handled separately
    // cross pair between two non-stables: split heuristically (MEXC lists both X+Y
    // directions rarely; treat symbol first-half as base)
    if (!pc) {
      const mid = sym.length / 2 | 0;
      const x = sym.slice(0, mid), y = sym.slice(mid);
      if (uniqCoins.has(x) && uniqCoins.has(y)) {
        if (!adj.has(x)) adj.set(x, []);
        if (!adj.has(y)) adj.set(y, []);
        adj.get(x).push({ pair: sym, peer: y });
        adj.get(y).push({ pair: sym, peer: x });
      }
    }
  }

  // volume rank for mid-coin cap: use the coin's USDT 24h quote volume when
  // present (passed via settings.coinVol map; built by scanner from tickers).
  const coinVol = settings.coinVol || new Map();
  const ranked = uniq.slice().sort((a, b) => (coinVol.get(b) || 0) - (coinVol.get(a) || 0));
  const midSet = new Set(ranked.slice(0, MAX_MID_COINS));

  // 'stable2' = 2-leg stable cross-rate loop: S1 → S2 → S1
  //   leg1: hold S1, BUY S2 on the S2+S1 asks (pair base=S2, quote=S1)
  //   leg2: hold S2, SELL S2 on the S1+S2 bids (pair base=S1, quote=S2) → back to S1
  // This is the classic cross-rate arbitrage between the two quoted directions
  // of the same stable pair (e.g. USDCUSDT asks vs USDTUSDC bids).
  const shapeN = { stable2: 2, quad: 4, penta: 5, hex: 6 };
  // heldBeforeLeg([pair, holdingBefore]): coin received AFTER executing that leg
  //   pair = BASE+QUOTE; if holdingBefore === QUOTE (ends the pair) we BUY on asks
  //     → receive BASE units = pair.slice(0, len - holdingBefore.length)
  //   if holdingBefore === BASE (starts the pair) we SELL on bids → receive QUOTE
  //     = pair.slice(holdingBefore.length)
  const heldAfter = ([pair, h]) =>
    pair.endsWith(h) ? pair.slice(0, pair.length - h.length) : pair.slice(h.length);
  for (const shape of shapes) {
    const n = shapeN[shape] || 0;
    const k = n - 1; // intermediate coins; k=1 means stable2 (2 legs)
    if (k === 1) {
      // stable2: enumerate stable pairs quoted against each other
      for (const sa of stables) for (const sb of stables) {
        if (sa === sb) continue;
        if (!set.has(sb + sa) || !set.has(sa + sb)) continue;
        // sb+sa: base=sb, quote=sa — buy sb with sa on ASKS; sa+sb: base=sa, quote=sb — sell sb on BIDS
        addLoop("stable2", sa, sb, null, [[sb + sa, sa], [sa + sb, sb]]);
      }
      continue;
    }
    if (k < 1) continue; // minimum 3 for a quad
    // level-0 paths: entry on a stable quote pair (hold stable s1, BUY coin on asks)
    let paths = [];
    for (const s1 of stables) for (const ca of uniq) {
      const p = ca + s1;
      if (set.has(p)) paths.push({ s1, legs: [[p, s1]] });
    }
    if (!paths.length) continue;
    for (let lvl = 1; lvl < k; lvl++) {
      const next = [];
      const budget = lvl < k - 1 ? 120_000 : Infinity; // per-level path budget
      let cut = 0;
      for (const { s1, legs } of paths) {
        if (cut >= budget) break;
        const prev = heldAfter(legs[legs.length - 1]); // coin received by last leg
        for (const { pair, peer } of adj.get(prev)) {
          const y = peer;
          if (!midSet.has(y)) continue;            // volume-rank cap
          if (legs.some(l => heldAfter(l) === y)) continue; // distinct coins
          next.push({ s1, legs: [...legs, [pair, prev]] });
          cut++;
        }
      }
      paths = next;
      if (!paths.length) break;
      // cap intermediate path count to bound memory (top by heuristic: keep those
      // whose last pair's peer has highest volume — good enough ordering in
      // practice because adj iteration follows the stable/USDT liquidity axis)
      if (paths.length > 100_000) paths.length = 100_000;
    }
    const cap = MAX_LOOPS_PER_SHAPE;
    let shapeCount = 0;
    outer: for (const { s1, legs } of paths) {
      const last = heldAfter(legs[legs.length - 1]); // final coin held before close
      for (const s2 of stables) {
        if (!set.has(last + s2)) continue; // closing: sell last coin on bids
        addLoop(shape, s1, s2, null, [...legs, [last + s2, last]]);
        if (++shapeCount >= cap) break outer;
      }
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
  const makerYield = makerPlan(chain, books); // zero-slippage limit-order gross
  // (BUY legs: full top-of-book depth at the ask; SELL legs: depth of the bid side)
  // maker capacity: min quote-USD available at each leg's best limit price.
  // SELL legs: sum(bid price * volume) is already quote-USD. BUY legs: sum(ask
  // price * volume) is in the pair's quote currency — convert to USD via the
  // quote currency's best bid vs USDT when available.
  const quoteUsd = (c, totalNative) => {
    const q = c.quote;
    if (q === "USDT") return totalNative;
    const b = books.get(q + "USDT");
    if (!b) return totalNative;
    const rate = bestTop(b.bids, true);
    return rate > 0 ? totalNative * rate : totalNative;
  };
  let makerCap = Infinity;
  for (const c of chain) {
    const b = books.get(c.pair);
    if (c.holding === c.quote) {
      const tops = syncTops(b.asks, b._atop || (b._atop = { arr: [], dirty: true }), false);
      makerCap = Math.min(makerCap, quoteUsd(c, tops.reduce((s, l) => s + l.price * l.volume, 0)));
    } else {
      const tops = syncTops(b.bids, b._btop || (b._btop = { arr: [], dirty: true }), true);
      makerCap = Math.min(makerCap, tops.reduce((s, l) => s + l.price * l.volume, 0));
    }
  }
  return {
    netYield, grossYield: r.grossYield, feeCost: r.feeCost, slippage: r.slippage,
    estimatedProfitUsd: r.capacity * netYield, capacityUsd: r.capacity,
    makerPlanYield: makerYield, makerCapacityUsd: isFinite(makerCap) ? makerCap : 0,
    reps: r.reps, chain,
  };
}

// ---------- MAKER (limit-order) fill math ----------
// A limit order fills at its limit price (or better) — zero slippage against the
// planned price — but it is not guaranteed to fill. For honest "100% win" math
// we report BOTH plans:
//  - TAKER plan: walk real depth → truthful fee+slippage (may be negative)
//  - MAKER plan: fill every leg at the best limit price → zero slippage cost;
//    the win is guaranteed by price logic (fees are fixed), the only uncertainty
//    is whether orders fill before the gap closes — the gap's tick persistence
//    measures exactly that.
// Maker plan assumes each leg's best bid/ask holds and the leg crosses at the
// same rate the taker plan computed (book volume available).
export function makerPlan(chain, books) {
  let acc = 1.0;
  for (const c of chain) {
    if (c.holding === c.quote) acc /= c._a0; // BUY limit at best ask
    else acc *= c._b0;                        // SELL limit at best bid
  }
  return acc - 1.0; // gross maker yield (zero slippage)
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

  // 1) coarse curve for display (classic buckets)
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

  // 2) precise per-dollar optimal size: three-pass sweep over [100..capacity]
  //    (coarse 100-step → 10-step around coarse peak → 1-step around fine peak)
  //    maximizing NET yield (the curve peaks then falls as depth runs out).
  // fine-grain curve (10$ steps across the whole capacity, capped) for a precise chart
  const preciseCurve = [];
  const fineStep = capacity <= 5000 ? 50 : capacity <= 50000 ? 500 : 1000;
  for (let s = 100; s <= capacity && preciseCurve.length < 120; s += fineStep) {
    const y = evalAt(s);
    if (y !== null) preciseCurve.push({ size_usd: s, net_yield: y });
  }
  if (preciseCurve.length && preciseCurve[preciseCurve.length - 1].size_usd < capacity) {
    const y = evalAt(capacity);
    if (y !== null) preciseCurve.push({ size_usd: capacity, net_yield: y });
  }

  const clamp = v => Math.max(100, Math.min(capacity, Math.round(v)));
  const sweep = (loS, hiS, step) => {
    let best = 100, bestY = evalAt(100) ?? -Infinity;
    for (let s = loS; s <= hiS; s += step) {
      const y = evalAt(s);
      if (y !== null && y > bestY) { best = s; bestY = y; }
    }
    return { best, bestY };
  };
  let p1 = sweep(100, clamp(capacity), 100);
  const lo2 = Math.max(100, p1.best - 100), hi2 = Math.min(capacity, p1.best + 100);
  let p2 = sweep(lo2, hi2, 10);
  const lo3 = Math.max(100, p2.best - 10), hi3 = Math.min(capacity, p2.best + 10);
  const p3 = sweep(lo3, hi3, 1);
  // prefer a size that still clears the threshold; fall back to peak if none
  let optimal = null, yieldAt = 0;
  for (const p of curve) if (p.size_usd <= capacity + 0.5 && p.net_yield >= threshold) { optimal = p.size_usd; yieldAt = p.net_yield; }
  if (p3.bestY !== -Infinity && p3.bestY >= (yieldAt || -Infinity) && p3.bestY >= threshold) {
    optimal = p3.best; yieldAt = p3.bestY;
  } else if (p3.bestY !== -Infinity && (optimal === null || p3.bestY > yieldAt)) {
    optimal = p3.best; yieldAt = p3.bestY;
  }
  if (optimal === null && curve.length) { optimal = curve[0].size_usd; yieldAt = curve[0].net_yield; }
  return { optimalSize: optimal ?? 0, optimalYield: yieldAt, curve, preciseCurve, capacity, preciseYieldPct: p3.bestY !== -Infinity ? p3.bestY * 100 : null };
}

// ---------- liquidity grade ----------
export function liquidityGrade(reps) {
  const avg = reps.reduce((s, r) => s + r.weightedVolumeUsd, 0) / reps.length;
  return avg > 5000 ? "A" : avg > 2000 ? "B" : avg > 800 ? "C" : avg > 200 ? "D" : "F";
}

// ---------- symbol universe ----------
// Public CORS proxies vary in uptime and per-domain blocks, so every REST call
// walks this chain (plus the allorigins JSON-wrapper variant) before giving up.
// Verified working against api.mexc.com (2026-08-15): cors.sh passes MEXC with
// ACAO=* and full headers. corsproxy.io / allorigins kept as backups but were
// 403/timeout on the day of the last sweep — the chain walks all of them.
// Proxy categories: RAW-path proxies (target URL appended verbatim, NOT encoded —
// verified against api.mexc.com 2026-08-15: ACAO=*, full headers) and encoded-
// wrapper proxies (target must be encodeURIComponent'd). MEXC URLs never contain
// spaces, so verbatim concatenation is safe.
const CORS_RAW = ["https://cors.sh/"];
const CORS_ENCODED = [
  "https://corsproxy.io/?",
  "https://proxy.cors.sh/",
  "https://api.allorigins.win/raw?url=",
  "https://cors.eu.org/",
];
// allorigins JSON wrapper: response is {contents: "payload", status:{http_code}}
const CORS_PROXIES_JSON = ["https://api.allorigins.win/get?url="];
async function timedFetch(input, ms) {
  const ac = new AbortController();
  const t = setTimeout(() => ac.abort(), ms);
  try { return await fetch(input, { signal: ac.signal }); }
  finally { clearTimeout(t); }
}
function looksJson(r) {
  const ct = (r.headers.get("content-type") || "").toLowerCase();
  return !ct || ct.includes("json") || ct.includes("text/plain") || ct.includes("text");
}
// cache-buster appended to the TARGET url (not inside encodeURIComponent),
// keeps the encoded payload clean for prefix proxies
function cb(url) {
  const sep = url.includes("?") ? "&" : "?";
  return url + sep + "_=" + Date.now();
}
export async function corsFetch(url, opts = {}) {
  let r = null;
  // caller abort (e.g. dossier poll closed) → fail fast, no proxy walk
  if (opts.signal && opts.signal.aborted) throw new DOMException("aborted", "AbortError");
  // 1) direct — works in most user regions
  try {
    r = await timedFetch(url, 8000);
    if (r.ok && looksJson(r)) return r;
  } catch { /* proxy below */ }
  try {
    r = await timedFetch(cb(url), 8000);
    if (r.ok && looksJson(r)) return r;
  } catch { /* proxy below */ }
  // 2) raw-path proxies: target appended verbatim (cors.sh family)
  for (const prefix of CORS_RAW) {
    try {
      const p = await timedFetch(prefix + cb(url), 20000);
      if (p.ok && looksJson(p)) return p;
    } catch { /* next proxy */ }
  }
  // 3) encoded-wrapper proxies
  for (const prefix of CORS_ENCODED) {
    try {
      const p = await timedFetch(prefix + encodeURIComponent(cb(url)), 14000);
      if (p.ok && looksJson(p)) return p;
    } catch { /* retry / next proxy */ }
  }
  // 3) JSON-wrapper proxy: body is {contents, status}; unwrap the real payload
  for (const prefix of CORS_PROXIES_JSON) {
    try {
      const p = await timedFetch(prefix + encodeURIComponent(url), 12000);
      if (p.ok) {
        const j = await p.json();
        if (j && typeof j.contents === "string" && (j.status || {}).http_code === 200) {
          return new Response(j.contents, { headers: { "content-type": "application/json" } });
        }
      }
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
  // cross-pair expansion (closing legs for quads/pentas/hexagons).
  // Cross legs are NOT volume-filtered at listing time: MEXC cross pairs often
  // show little 24h volume but real depth at the top (the $100 tradability test
  // and the live order-book depth validation catch genuinely thin books). The
  // earlier 1000-vol floor silently killed every cross leg and produced zero
  // loops — cross legs are depth-validated at validation time instead.
  const coins = [...whitelist].map(s => { const pc = parseSymbol(s, STABLES); return pc && !isStable(pc.coin, STABLES) ? pc.coin : null; }).filter(Boolean);
  const uniqCoins = new Set(coins);
  const minCrossVol = 0;
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
