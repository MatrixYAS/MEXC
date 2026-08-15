// JS port of the scanner loop + MEXC protobuf WS feed — v2.
// Supports triangle AND quadrangle loops over the multi-stablecoin basket
// (USDT/USDC/DAI/FDUSD/TUSD). One loop = 2 (triangle) or 3 (quad) intermediate coins.
import { decodeDepthFrame } from "./pb.js";
import {
  newBookState, applyDelta, markDirty,
  validateLoop, findOptimalSize, liquidityGrade,
  buildLoops, buildUniverse, tradableScore, corsFetch,
  parseSymbol, isStable, STABLES, DEFAULTS,
} from "./engine.js";

const WS_URL = "wss://wbs-api.mexc.com/ws";
const MAX_PER_CONN = 30;
const RESNAP_INTERVAL = 120_000;

function shapeBadge(loop) {
  const n = loop.pairs.length;
  if (loop.shape === "stable2") return "STB2";
  return n === 4 ? "QUAD" : n === 5 ? "PENTA" : n === 6 ? "HEX" : "TRI";
}

export class Scanner {
  constructor(settings = DEFAULTS, onOpportunity, onStats) {
    this.settings = { ...DEFAULTS, ...settings };
    this.onOpportunity = onOpportunity;
    this.onStats = onStats;
    this.books = new Map();          // symbol -> book state
    this.loops = [];
    this.persistence = new Map();    // loop.key -> { firstSeen, consecutive, best }
    this.lastEmit = new Map();
    this.wsConns = [];
    this.firstBook = false;
    this.cleanupTick = 0;
    this.bestGap = null;
    this.running = false;
    this.paused = false;
    this.fallbackMode = false;
    this.start();
  }

  async start() {
    if (this.running) return;
    this.running = true;
    // universe is built fresh from the live MEXC 24h ticker each restart
    let symbols = null;
    try {
      symbols = await buildUniverse(this.settings.minVol || 300_000, this.settings.maxWhitelist || 450, this.settings.liquidityTestUsd ?? DEFAULTS.liquidityTestUsd, this.settings.maxSlip ?? DEFAULTS.maxSlip);
      this.coinLiquidity = symbols.liquidity || new Map();
      this.liquidityTestUsd = symbols.liquidityTestUsd ?? DEFAULTS.liquidityTestUsd;
      this.maxSlip = symbols.maxSlip ?? DEFAULTS.maxSlip;
      this.fallbackMode = false;
    } catch {
      this.fallbackMode = true;
      symbols = null;
    }
    if (!symbols || symbols.length === 0) {
      this.fallbackMode = true;
      symbols = this._hardFallbackSymbols();
    }
    // rank coins by 24h quote volume so the builder picks liquid intermediates,
    // and fetch EXACT base/quote splits from exchangeInfo so cross-pair edges
    // (e.g. BTCETH, DOGEKAS) are never mangled by string splitting
    let coinVol = null, baseQuote = null;
    try {
      const tv = await (await corsFetch("https://api.mexc.com/api/v3/ticker/24hr")).json();
      coinVol = new Map(tv.map(t => {
        const pc = parseSymbol(t.symbol, STABLES);
        const c = pc && !isStable(pc.coin, STABLES) ? pc.coin : null;
        return c ? [c, parseFloat(t.quoteVolume) || 0] : null;
      }).filter(Boolean));
    } catch {}
    try {
      const ex = await (await corsFetch("https://api.mexc.com/api/v3/exchangeInfo")).json();
      baseQuote = new Map(ex.symbols.map(s => [s.symbol, { base: s.baseAsset, quote: s.quoteAsset }]));
    } catch {}
    this.loops = buildLoops(symbols, { ...this.settings, coinVol, baseQuote });
    this._connectSymbols(symbols);
    this.reSnapTimer = setInterval(() => this._reseedAll(), RESNAP_INTERVAL);
    this.scanTimer = setInterval(() => this._tick(), 50);
    // if the startup universe build failed (e.g. regional API block), fall back
    // to measuring tradability ourselves from the live books so the Coins page
    // and the loop universe still reflect reality
    if ((this.coinLiquidity || new Map()).size === 0) this._measureLiquidityFromBooks();
  }

  stop() {
    this.running = false;
    clearInterval(this.reSnapTimer); clearInterval(this.scanTimer);
    for (const c of this.wsConns) { try { c.close(); } catch {} }
    this.wsConns = [];
  }

  _hardFallbackSymbols() {
    return ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "AVAXUSDT", "DOTUSDT", "LINKUSDT", "LTCUSDT", "BCHUSDT", "TRXUSDT", "NEARUSDT", "APTUSDT", "ARBUSDT", "OPUSDT", "ATOMUSDT", "FILUSDT", "USDCUSDT", "ETHUSDC", "SOLUSDC", "XRPUSDC", "ADAUSDC", "AVAXUSDC", "DOTUSDC", "LINKUSDC", "LTCUSDC", "BCHUSDC", "TRXUSDC", "NEARUSDC", "APTUSDC", "ARBUSDC", "OPUSDC", "ATOMUSDC", "FILUSDC", "ETHBTC", "SOLBTC", "XRPBTC", "DOGEBTC", "ADABTC", "AVAXBTC", "DOTBTC", "LINKBTC", "LTCBTC", "BCHBTC", "TRXBTC", "NEARBTC", "APTBTC", "ARBBTC", "OPBTC", "ATOMBTC", "FILBTC", "BTCETH", "SOLETH", "XRPETH", "DOGEETH", "ADAETH", "AVAXETH", "DOTETH", "LINKETH", "LTCETH", "BCHETH", "TRXETH", "NEARETH", "APTETH", "ARBETH", "OPETH", "ATOMETH", "FILETH", "SOLBANK", "XRPBANK", "DOGEBANK", "ADABANK", "AVAXBANK", "DOTBANK", "LINKBANK", "LTCBANK", "BCHBANK", "TRXBANK", "NEARBANK", "APTBANK", "ARBBANK", "OPBANK", "ATOMBANK", "FILBANK", "BTCBANK", "ETHBANK", "USDCBANK", "USDTBANK", "BANKUSDC", "BANKUSDT", "ADABANK", "BANKADA", "BANKDOT", "DOTBANK", "BANKSOL", "SOLBANK", "BANKLINK", "LINKBANK"];
  }

  _connectSymbols(symbols) {
    const chunks = [];
    for (let i = 0; i < symbols.length; i += MAX_PER_CONN) chunks.push(symbols.slice(i, i + MAX_PER_CONN));
    for (let w = 0; w < chunks.length; w++) this._spawnWorker(chunks[w], w);
  }

  _spawnWorker(symbols, workerId) {
    const doConnect = async () => {
      while (this.running) {
        try {
          const ws = new WebSocket(WS_URL);
          ws.binaryType = "arraybuffer";
          this.wsConns.push(ws);
          await new Promise((res, rej) => {
            ws.onopen = () => res();
            ws.onerror = rej;
            ws.onmessage = (ev) => this._onMessage(ev.data);
            setTimeout(() => rej(new Error("ws handshake timeout")), 15000);
          });
          ws.send(JSON.stringify({ method: "SUBSCRIPTION", params: symbols.map(s => `spot@public.aggre.depth.v3.api.pb@100ms@${s}`) }));
          for (const sym of symbols) { if (!this.books.has(sym)) { const s = newBookState(); this.books.set(sym, s); } }
          this._reseed(symbols);
          this._emitStats();
          await new Promise(res => { ws.onclose = () => res(); ws.onerror = () => res(); });
        } catch (e) { /* reconnect */ }
        if (!this.running) break;
        await sleep(3000);
      }
    };
    doConnect();
  }

  _onMessage(data) {
    try {
    if (data instanceof ArrayBuffer || data.byteLength !== undefined) {
      const depth = decodeDepthFrame(data);
      if (!depth || !depth.channel.includes("aggre.depth")) return;
      const sym = depth.symbol.toUpperCase();
      const book = [...this.books.keys()].find(k => k.toUpperCase() === sym);
      if (!book) return;
      const b = this.books.get(book);
      for (const [price, qty] of depth.bids) applyDelta(b.bids, price, qty);
      for (const [price, qty] of depth.asks) applyDelta(b.asks, price, qty);
      markDirty(b);
      b.lastUpdate = Date.now();
      if (!this.firstBook) { this.firstBook = true; this._emitStats(); }
    }
    } catch (e) { /* never let a bad frame kill the worker */ }
  }
  _emitStats() {
    if (!this.onStats) return;
    const coinsSnap = [...(this.coinLiquidity || new Map()).values()].map(t => ({
      coin: t.coin, pair: t.pair, vol24h: t.vol24h,
      depthUsd: t.depthUsd,
      buySlipPct: t.buy ? t.buy.slip * 100 : null,
      sellSlipPct: t.sell ? t.sell.slip * 100 : null,
    }));
    this.onStats({
      coins: coinsSnap,
      liquidityTestUsd: this.liquidityTestUsd,
      maxSlipPct: this.maxSlip * 100,
      loops: this.loops.length,
      books: this.books.size,
      measuring: !this.firstBook,
      bestGrossYieldPct: null,
      bestGapPath: null,
      pending: 0,
    });
  }

  async _reseed(symbols) {
    for (const sym of symbols) {
      // REST seed walked through the full CORS fallback chain (direct, then
      // 6 public proxies); failures tolerated — the 100ms WebSocket deltas
      // converge the books within seconds anyway
      let j = null;
      const url = `https://api.mexc.com/api/v3/depth?symbol=${sym}&limit=100`;
      try {
        const r = await corsFetch(url);
        if (r && r.ok) j = await r.json();
      } catch { /* next symbol — WS deltas will converge the book */ }
      if (!j) continue;
      const b = this.books.get(sym) || (() => { const s = newBookState(); this.books.set(sym, s); return s; })();
      for (const [p, q] of j.bids || []) { if (parseFloat(q) > 0) b.bids.set(p, parseFloat(q)); }
      for (const [p, q] of j.asks || []) { if (parseFloat(q) > 0) b.asks.set(p, parseFloat(q)); }
      markDirty(b);
      b.lastUpdate = Date.now();
      await sleep(80); // respect IP rate limit (~10/s total)
    }
  }

  async _reseedAll() { if (this.running) await this._reseed([...this.books.keys()]); }

  async _measureLiquidityFromBooks() {
    // runs once, in the background, on the live-seeded books
    const usd = this.settings.liquidityTestUsd ?? DEFAULTS.liquidityTestUsd;
    const maxSlip = this.settings.maxSlip ?? DEFAULTS.maxSlip;
    this.coinLiquidity = new Map();
    this.liquidityTestUsd = usd;
    this.maxSlip = maxSlip;
    const seen = new Map(); // coin -> {symbol, vol-like depth}
    for (const [sym, b] of this.books) {
      const pc = parseSymbol(sym, STABLES);
      const c = pc && pc.st ? pc.coin : null;
      if (!c || isStable(c, STABLES)) continue;
      // keep the deepest stable pair per coin as its tradability representative
      const prev = seen.get(c);
      if (prev) {
        const prevSize = (this.books.get(prev)?.bids.size || 0) + (this.books.get(prev)?.asks.size || 0);
        const curSize = b.bids.size + b.asks.size;
        if (curSize <= prevSize) continue;
      }
      seen.set(c, sym);
    }
    const jobs = [];
    for (const [c, sym] of seen) {
      const b = this.books.get(sym);
      if (!b) continue;
      jobs.push((async () => {
        // use the live-seeded snapshot; re-seed with a REST top-up first if empty
        if (b.bids.size === 0 || b.asks.size === 0) await this._reseed([sym]);
        if (b.bids.size === 0 || b.asks.size === 0) return;
        const t = tradableScore(b, usd, maxSlip);
        if (!t.ok) return;
        const depthUsd = Math.min((t.buy?.depthUsd || 0), (t.sell?.depthUsd || 0));
        this.coinLiquidity.set(c, { coin: c, pair: sym, buy: t.buy, sell: t.sell, ok: t.ok, depthUsd, vol24h: 0 });
      })());
    }
    await Promise.all(jobs);
    // fall back to the hardcoded list of known major coins if measurement found none
    if (this.coinLiquidity.size === 0) {
      for (const sym of ["BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "TRXUSDT", "ADAUSDT", "AVAXUSDT", "DOTUSDT", "LINKUSDT", "LTCUSDT", "BCHUSDT", "NEARUSDT", "APTUSDT", "ATOMUSDT", "FILUSDT", "SUIUSDT", "PEPEUSDT", "SHIBUSDT"]) {
        const b = this.books.get(sym);
        if (!b) continue;
        const pc = parseSymbol(sym, STABLES);
        if (!pc || !pc.coin) continue;
        const t = tradableScore(b, usd, maxSlip);
        if (t.ok) this.coinLiquidity.set(pc.coin, { coin: pc.coin, pair: sym, buy: t.buy, sell: t.sell, ok: t.ok, depthUsd: Math.min(t.buy.depthUsd, t.sell.depthUsd), vol24h: 0 });
      }
    }
    this._emitStats();
  }

  _tick() {
    if (this.paused || !this.running) return;
    const now = Date.now();
    let sampleGap = null;
    const emitCooldown = this.settings.emitCooldownMs || 5000;
    for (const loop of this.loops) {
      // all books must exist and be recent; validateLoop also checks <2s freshness
      let allOk = true;
      for (const [pair] of loop.legs) {
        const b = this.books.get(pair);
        if (!b || now - (b.lastUpdate || 0) > 2000) { allOk = false; break; }
      }
      if (!allOk) continue;

      const report = validateLoop(loop, this.books, this.settings);
      const state = this.persistence.get(loop.key) || { firstSeen: now, consecutive: 0, best: null };
      if (report && report.netYield >= this.settings.minProfitThreshold) {
        state.consecutive = (state.consecutive || 0) + 1;
        if (!state.best || report.netYield > state.best.netYield) state.best = report;
        state.lastSeen = now;
      } else {
        state.consecutive = 0; state.best = null;
      }
      state.firstSeen = state.firstSeen || now;
      this.persistence.set(loop.key, state);

      if (!report) continue;
      // track best live gross gap even below threshold
      if (!sampleGap || report.grossYield > sampleGap.grossYield)
        sampleGap = { y: report.grossYield, path: loop.display };

      if (state.consecutive < this.settings.requiredTicks) continue;
      const lastE = this.lastEmit.get(loop.key) || 0;
      if (now - lastE < emitCooldown) continue;

      // freshness gate: every leg book < 1s old at emission
      let age = 0;
      for (const [pair] of loop.legs) { const u = this.books.get(pair).lastUpdate || 0; age = Math.max(age, now - u); }
      if (age > 1000) continue;

      const fr = state.best || report;
      const grade = liquidityGrade(fr.reps);
      const gradeScore = { A: 1.0, B: 0.85, C: 0.7, D: 0.55 }[grade.charAt(0)] || 0.3;
      const ticksScore = Math.min(state.consecutive / 10, 1);
      const confidence = (gradeScore * 0.6 + ticksScore * 0.4) * 100;

      const sizing = findOptimalSize(loop, this.books, this.settings) || { optimalSize: 0, optimalYield: 0, curve: [] };
      const opt = sizing.optimalSize > 0 ? sizing.optimalSize : 100;
      const estProfit = opt * sizing.optimalYield;

      this.lastEmit.set(loop.key, now);
      const legs = fr.reps.map(r => ({
        symbol: r.pair,
        entryPrice: r.entry,
        fillPrice: r.fill,
        weightedVolumeUsd: r.weightedVolumeUsd,
        slippagePct: r.slippagePct * 100,
        direction: r.direction,
      }));
      this.onOpportunity({
        id: loop.key,
        shape: shapeBadge(loop),
        nLegs: loop.pairs.length,
        path: loop.display,
        netYieldPercent: fr.netYield * 100,
        grossGapPercent: fr.grossYield * 100,
        feeCostPercent: fr.feeCost * 100,
        estimatedProfitUsd: estProfit,
        capacityUsd: opt,
        gapAgeMs: now - state.firstSeen,
        ticksSurvived: state.consecutive,
        fillScore: grade,
        stalenessMs: age,
        confidence,
        makerPlanYieldPercent: fr.makerPlanYield * 100,
        slippagePercent: fr.slippage * 100,
        optimalSizeUsd: opt,
        optimalNetYieldPercent: sizing.optimalYield * 100,
        sizeCurveJson: JSON.stringify(sizing.curve),
        legs,
        detectedAt: new Date().toISOString(),
      });
    }

    // lightweight stale-entry cleanup every 20 ticks (~1s)
    if (++this.cleanupTick % 20 === 0) {
      for (const [k, s] of this.persistence) if (now - (s.lastSeen || 0) > 30000) this.persistence.delete(k);
    }

    if (sampleGap) {
      if (!this.bestGap || sampleGap.y > this.bestGap.y || now - this.bestGap.t > 5000)
        this.bestGap = { y: sampleGap.y, path: sampleGap.path, t: now };
    }
    const coinsSnap = [...(this.coinLiquidity || new Map()).values()].map(t => ({
      coin: t.coin, pair: t.pair, vol24h: t.vol24h,
      depthUsd: t.depthUsd,
      buySlipPct: t.buy ? t.buy.slip * 100 : null,
      sellSlipPct: t.sell ? t.sell.slip * 100 : null,
    }));
    this.onStats({
      coins: coinsSnap,
      liquidityTestUsd: this.liquidityTestUsd,
      maxSlipPct: this.maxSlip * 100,
      loops: this.loops.length,
      books: this.books.size,
      bestGrossYieldPct: this.bestGap ? (this.bestGap.y - 1) * 100 : null,
      bestGapPath: this.bestGap ? this.bestGap.path : null,
      pending: [...this.persistence.values()].filter(s => s.consecutive > 0 && s.consecutive < this.settings.requiredTicks).length,
    });
  }
}

const sleep = (ms) => new Promise(r => setTimeout(r, ms));
