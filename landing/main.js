// menger.sh — DESIGN-37 (MENGER-7, MENGER-10 beneath). Progressive enhancement
// only: the page reads whole without this file. It (1) points the Log in links
// at the app named by <html data-app-url>, (2) runs the boot, (3) lights the
// wall — the Menger carpet in the lifecycle's colours — and leans its cells
// toward the cursor, (4) types the install line, (5) reveals sections on view
// and develops pictures as they arrive, (6) runs the pinned story, (7) counts
// the counters and types the terminal, (8) offers the visitor's OS first in
// the Download section, (9) adds copy buttons to commands, (10) plays the
// clips beside the principles while they are on screen, (11) runs the tour's
// chapters. Shared by the landing page and guide/. It requests nothing but
// the page's own media, and only as it nears the screen. No inline style:
// every value it sets goes through the CSSOM (the CSP allows no attribute).
(function () {
  "use strict";
  var root = document.documentElement;
  root.classList.add("js");

  // (1) the hosted app, set in one place
  var APP_URL = (root.getAttribute("data-app-url") || "https://app.menger.sh").replace(/\/+$/, "");
  document.querySelectorAll("a[data-login]").forEach(function (a) { a.href = APP_URL + "/login"; });

  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
  function near(el, fn, margin, threshold) {
    if (!("IntersectionObserver" in window)) { fn(true); return; }
    new IntersectionObserver(function (es) { es.forEach(function (e) { fn(e.isIntersecting); }); }, { rootMargin: margin || "0px", threshold: threshold || 0 }).observe(el);
  }
  function once(el, fn, margin, threshold) {
    if (!("IntersectionObserver" in window)) { fn(); return; }
    var io = new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting) { io.unobserve(e.target); fn(); } }); }, { rootMargin: margin || "0px", threshold: threshold || 0 });
    io.observe(el);
  }
  function count(b, n, ms) {
    if (reduce.matches) { b.textContent = n; return; }
    var t0 = performance.now();
    (function tick() { var p = Math.min(1, (performance.now() - t0) / ms); b.textContent = Math.round(n * (1 - Math.pow(1 - p, 3))); if (p < 1) requestAnimationFrame(tick); })();
  }

  // (2) the boot: the wordmark's cells light while a count runs to 512 ------
  var boot = document.getElementById("boot");
  if (boot) {
    if (reduce.matches) { boot.remove(); }
    else {
      var cells = Array.prototype.slice.call(boot.querySelectorAll("rect"));
      cells.forEach(function (r, i) { setTimeout(function () { r.classList.add("on"); }, 120 + i * 90); });
      count(document.getElementById("boot-n"), 512, 1100);
      setTimeout(function () { document.getElementById("boot-w").textContent = "one plan"; }, 1100);
      setTimeout(function () { boot.classList.add("done"); }, 1350);
      setTimeout(function () { boot.remove(); }, 2100);
    }
  }

  // (3) the wall ------------------------------------------------------------
  // The Menger carpet: a square divided into nine, its centre removed, again
  // and again. A scope (level 1) divides into 8, 64, 512 pieces of work; the
  // leaves are Pending, then the work moves them through Ready, Coding and
  // In Review to Complete — 64 chains of 8, serial inside the smallest block
  // and parallel above it, the starts coming ever faster. Each cell takes the
  // colour of its status; ten cells carry an id and the state of the block
  // they are in. Held a moment, the cycle begins again.
  var NS = "http://www.w3.org/2000/svg";
  var wall = document.getElementById("wall");
  function inCarpet(x, y) { while (x > 0 || y > 0) { if (x % 3 === 1 && y % 3 === 1) return false; x = Math.floor(x / 3); y = Math.floor(y / 3); } return true; }
  function squares(level) { var n = Math.pow(3, level), s = 27 / n, out = []; for (var y = 0; y < n; y++) for (var x = 0; x < n; x++) if (inCarpet(x, y)) out.push({ x: x * s, y: y * s, s: s, gx: x, gy: y }); return out; }
  var BLOCK = { "0,0": 0, "1,0": 1, "2,0": 2, "2,1": 3, "2,2": 4, "1,2": 5, "0,2": 6, "0,1": 7 };
  function rankOf(sq) {
    var b1 = BLOCK[Math.floor(sq.gx / 9) + "," + Math.floor(sq.gy / 9)];
    var b2 = BLOCK[(Math.floor(sq.gx / 3) % 3) + "," + (Math.floor(sq.gy / 3) % 3)];
    var b3 = BLOCK[(sq.gx % 3) + "," + (sq.gy % 3)];
    return b1 * 64 + b2 * 8 + b3;
  }
  function rect(sq, cls) {
    var g = Math.min(0.1 * sq.s, 0.5), r = document.createElementNS(NS, "rect");
    r.setAttribute("x", (sq.x + g / 2).toFixed(3)); r.setAttribute("y", (sq.y + g / 2).toFixed(3));
    r.setAttribute("width", (sq.s - g).toFixed(3)); r.setAttribute("height", (sq.s - g).toFixed(3));
    r.setAttribute("rx", sq.s > 2 ? ".18" : ".08"); r.setAttribute("class", cls);
    r._cx = sq.x + sq.s / 2; r._cy = sq.y + sq.s / 2;
    return r;
  }
  if (wall) {
    while (wall.firstChild) wall.removeChild(wall.firstChild);
    var l1 = squares(1).map(function (sq) { return rect(sq, "sq scope enter"); });
    var l2 = squares(2).map(function (sq) { return rect(sq, "sq enter"); });
    var l3 = squares(3).map(function (sq) { var r = rect(sq, "sq enter"); r._rank = rankOf(sq); r._block = Math.floor(r._rank / 8); return r; });
    l1.concat(l2, l3).forEach(function (r) { wall.appendChild(r); });
    var IDS = ["OFFLINE-2", "OFFLINE-4", "SYNC-3", "OFFLINE-7", "OFFLINE-9", "SYNC-2", "OFFLINE-10", "OFFLINE-6", "SYNC-4", "OFFLINE-12"];
    var ST = ["PENDING", "READY", "CODING", "IN REVIEW", "COMPLETE"], labelCells = [1, 5, 10, 14, 23, 29, 38, 44, 52, 60], labels = [], l2sq = squares(2);
    labelCells.forEach(function (i, k) {
      var sq = l2sq[i], t = document.createElementNS(NS, "text");
      t.setAttribute("class", "cell-label"); t.setAttribute("x", (sq.x + 0.28).toFixed(2)); t.setAttribute("y", (sq.y + 0.95).toFixed(2));
      var a = document.createElementNS(NS, "tspan"); a.textContent = IDS[k];
      var b = document.createElementNS(NS, "tspan"); b.setAttribute("class", "st"); b.setAttribute("x", (sq.x + 0.28).toFixed(2)); b.setAttribute("dy", ".8"); b.textContent = "PENDING";
      t.appendChild(a); t.appendChild(b); t.style.opacity = 0; wall.appendChild(t);
      labels.push({ t: t, st: b, block: Math.floor(rankOf({ gx: sq.gx * 3, gy: sq.gy * 3 }) / 8) });
    });
    var state = l3.map(function () { return 0; }), timers = [], running = false;
    function at(fn, ms) { timers.push(setTimeout(fn, ms)); }
    function clearTimers() { timers.forEach(clearTimeout); timers = []; }
    function setState(i, s) { state[i] = s; l3[i].setAttribute("class", "sq s" + s + (l3[i].classList.contains("near") ? " near" : "")); }
    function refreshLabels() {
      labels.forEach(function (L) { var min = 4; l3.forEach(function (r, i) { if (r._block === L.block && state[i] < min) min = state[i]; }); L.st.textContent = ST[min]; });
    }
    function finalState() {
      l1.forEach(function (r) { r.setAttribute("class", "sq scope gone"); }); l2.forEach(function (r) { r.setAttribute("class", "sq gone"); });
      l3.forEach(function (r, i) { setState(i, i % 7 === 0 ? 2 : i % 5 === 0 ? 1 : i % 11 === 0 ? 3 : 4); });
      labels.forEach(function (L) { L.t.style.opacity = 1; }); refreshLabels();
    }
    function cycle() {
      clearTimers();
      l1.forEach(function (r) { r.setAttribute("class", "sq scope enter"); }); l2.forEach(function (r) { r.setAttribute("class", "sq enter"); }); l3.forEach(function (r) { r.setAttribute("class", "sq enter"); });
      labels.forEach(function (L) { L.t.style.opacity = 0; });
      at(function () { l1.forEach(function (r) { r.setAttribute("class", "sq scope"); }); }, 2000);
      at(function () { l2.forEach(function (r, i) { r.style.setProperty("--d", (i * 9) + "ms"); r.setAttribute("class", "sq"); }); l1.forEach(function (r) { r.setAttribute("class", "sq scope gone"); }); }, 3000);
      at(function () { l3.forEach(function (r) { r.setAttribute("class", "sq s0"); }); l2.forEach(function (r) { r.setAttribute("class", "sq gone"); }); labels.forEach(function (L) { L.t.style.opacity = 1; }); }, 4200);
      var t0 = 5000;
      l3.forEach(function (r, i) {
        var b = r._block, w = r._rank % 8, start = t0 + b * (120 - b) * 0.9 + w * 260;
        for (var s = 1; s <= 4; s++) (function (s) { at(function () { setState(i, s); if (s === 4 || w === 0) refreshLabels(); }, start + s * 170); })(s);
      });
      at(cycle, t0 + 64 * 60 + 8 * 260 + 4 * 170 + 3000);
    }
    function start() { if (running || reduce.matches) return; running = true; cycle(); }
    function stop() { if (!running) return; running = false; clearTimers(); }
    function sync() { if (reduce.matches) { stop(); finalState(); return; } if (!document.hidden) start(); else stop(); }
    document.addEventListener("visibilitychange", sync);
    if (reduce.addEventListener) reduce.addEventListener("change", sync);
    if (reduce.matches) finalState(); else sync();
    // the cursor: cells within reach lean toward it
    if (!reduce.matches) {
      var pt = wall.createSVGPoint(), raf = 0, last = null;
      wall.addEventListener("mousemove", function (e) {
        last = e;
        if (!raf) raf = requestAnimationFrame(function () {
          raf = 0; pt.x = last.clientX; pt.y = last.clientY;
          var m = wall.getScreenCTM(); if (!m) return;
          var p = pt.matrixTransform(m.inverse());
          l3.forEach(function (r) { var dx = r._cx - p.x, dy = r._cy - p.y, isNear = dx * dx + dy * dy < 7; if (isNear !== r.classList.contains("near")) r.classList.toggle("near", isNear); });
        });
      });
      wall.addEventListener("mouseleave", function () { l3.forEach(function (r) { r.classList.remove("near"); }); });
    }
  }

  // (4) the install line types itself ---------------------------------------
  var typed = document.getElementById("typed");
  if (typed && !reduce.matches) {
    var line = typed.getAttribute("data-line") || typed.textContent, i = 0;
    typed.textContent = ""; typed.classList.add("caret");
    setTimeout(function step() { typed.textContent = line.slice(0, ++i); if (i < line.length) setTimeout(step, 28 + Math.random() * 40); else setTimeout(function () { typed.classList.remove("caret"); }, 1800); }, 2300);
  }

  // (5) reveal on view; pictures develop as they arrive ----------------------
  var reveals = document.querySelectorAll(".reveal, .develop");
  if ("IntersectionObserver" in window && !reduce.matches) {
    var ro = new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); ro.unobserve(e.target); } }); }, { rootMargin: "0px 0px -8% 0px" });
    reveals.forEach(function (el) { ro.observe(el); });
  } else {
    reveals.forEach(function (el) { el.classList.add("in"); });
  }

  // (6) the pinned story: the step follows the scroll; OFFLINE-10 moves ------
  var pin = document.getElementById("pin");
  if (pin) {
    var steps = pin.querySelectorAll(".step"), prog = document.getElementById("prog"), st = document.getElementById("st"), cap = document.getElementById("cap"), hot = pin.querySelector("[data-ticket]");
    var STATES = [
      ["st-pending", "Pending", "the board — <b>OFFLINE-10</b> is Pending: filed, not yet promoted"],
      ["st-ready", "Ready", "<b>OFFLINE-10</b> promoted to Ready by the owner; an agent claims it: Coding"],
      ["st-complete", "Complete", "the commit linked, the review merged — <b>OFFLINE-10</b> is Complete on every screen"]
    ];
    var TONES = ["var(--pending)", "var(--ready)", "var(--complete)"], cur = 0;
    pin.style.setProperty("--step-tone", TONES[0]);
    function setStep(k) {
      if (k === cur) return; cur = k;
      pin.style.setProperty("--step-tone", TONES[k]);
      steps.forEach(function (s, i) { s.classList.toggle("on", i === k); });
      st.className = "chip " + STATES[k][0] + " swap"; st.textContent = STATES[k][1]; cap.innerHTML = STATES[k][2];
      hot.classList.add("hot");
      setTimeout(function () { hot.classList.remove("hot"); st.classList.remove("swap"); }, 700);
    }
    function onScroll() {
      var r = pin.getBoundingClientRect(), total = r.height - window.innerHeight; if (total <= 0) return;
      var p = Math.min(1, Math.max(0, (-r.top) / total));
      prog.style.width = (p * 100) + "%";
      setStep(p < 0.34 ? 0 : p < 0.67 ? 1 : 2);
    }
    if (window.matchMedia("(min-width: 860px)").matches) { window.addEventListener("scroll", onScroll, { passive: true }); onScroll(); }
    else { steps.forEach(function (s) { s.classList.add("on"); }); }
  }

  // (7) counters count up; the terminal types as it comes into view ----------
  document.querySelectorAll(".stats").forEach(function (el) {
    once(el, function () { el.querySelectorAll("[data-count]").forEach(function (b) { count(b, +b.getAttribute("data-count"), 1200); }); }, "0px", 0.2);
  });
  document.querySelectorAll("pre.cmd[data-type]").forEach(function (pre) {
    var code = pre.querySelector("code"); if (!code) return;
    pre.setAttribute("data-full", code.textContent);
    if (reduce.matches) return;
    var nodes = Array.prototype.slice.call(code.childNodes).map(function (n) { return { cls: n.nodeType === 1 ? n.className : "", text: n.textContent }; });
    code.textContent = "";
    once(pre, function () {
      var i = 0, j = 0, span = null;
      (function step() {
        if (i >= nodes.length) return;
        if (!span) { span = document.createElement("span"); if (nodes[i].cls) span.className = nodes[i].cls; code.appendChild(span); }
        span.textContent = nodes[i].text.slice(0, ++j);
        if (j >= nodes[i].text.length) { i++; j = 0; span = null; }
        setTimeout(step, nodes[i] && (nodes[i].cls === "c" || nodes[i].cls === "o") ? 6 : 22);
      })();
    }, "0px", 0.3);
  });

  // (8) the visitor's OS first in Download -----------------------------------
  function detectOS() {
    var p = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "").toLowerCase();
    var ua = (navigator.userAgent || "").toLowerCase();
    if (/android|iphone|ipad|ipod/.test(ua)) return "mobile";
    if (/mac/.test(p) || /mac os x/.test(ua)) return "mac";
    if (/win/.test(p) || /windows/.test(ua)) return "windows";
    if (/linux|x11|cros/.test(p) || /linux|x11/.test(ua)) return "linux";
    return null;
  }
  var NAMES = { linux: "Linux", mac: "macOS", windows: "Windows" };
  var HINTS = {
    linux: "AppImage, .deb and .rpm — pick yours on the release page.",
    mac: "Apple Silicon (aarch64) or Intel (x64) .dmg — on the release page.",
    windows: "The x64 installer (.exe) or the .msi — on the release page."
  };
  var os = detectOS();
  var main = document.getElementById("dl-main"), hint = document.getElementById("dl-hint");
  if (os && NAMES[os]) {
    var card = document.querySelector('.os[data-os="' + os + '"]');
    if (card) { card.classList.add("detected"); card.parentNode.insertBefore(card, card.parentNode.firstChild); }
    if (main) main.textContent = "Download for " + NAMES[os] + " ↓";
    if (hint) hint.textContent = HINTS[os];
    document.querySelectorAll("a[data-download]").forEach(function (a) { a.textContent = (a.classList.contains("btn-small") ? "" : "Download for ") + (a.classList.contains("btn-small") ? "Download" : NAMES[os]); });
    if (os === "mac" && hint && navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
      navigator.userAgentData.getHighEntropyValues(["architecture"]).then(function (v) {
        if (v.architecture === "arm") hint.textContent = "Apple Silicon: the aarch64 .dmg, on the release page.";
        else if (v.architecture === "x86") hint.textContent = "Intel: the x64 .dmg, on the release page.";
      }, function () {});
    }
  } else if (os === "mobile" && hint) {
    hint.textContent = "The desktop app is for Linux, macOS and Windows; mobile apps are coming.";
  }

  // (9) copy buttons ----------------------------------------------------------
  if (navigator.clipboard && window.isSecureContext) {
    document.querySelectorAll("pre.cmd").forEach(function (pre) {
      var b = document.createElement("button");
      b.type = "button"; b.className = "copy"; b.textContent = "copy";
      b.setAttribute("aria-label", "Copy these commands");
      b.addEventListener("click", function () {
        // the commands only: no comments, no sample output — from the whole text, whatever has been typed
        var full = pre.getAttribute("data-full");
        var text;
        if (full) { text = full; }
        else { var code = pre.querySelector("code").cloneNode(true); code.querySelectorAll(".o, .c").forEach(function (n) { n.remove(); }); text = code.textContent; }
        text = text.split("\n").filter(function (l) { return l.trim() && !/^\s*#/.test(l) && !/^\s{2}/.test(l) && !/^summary:/.test(l); })
          .map(function (l) { return l.replace(/\s+#.*$/, ""); }).join("\n");
        navigator.clipboard.writeText(text).then(function () { b.textContent = "copied"; setTimeout(function () { b.textContent = "copy"; }, 1600); }, function () { b.textContent = "select to copy"; });
      });
      pre.appendChild(b);
    });
  }

  // (10) the principles' clips ---------------------------------------------------
  // Each has controls and no poster in the HTML (a poster is fetched at once,
  // wherever the video is): the poster is set as it nears the screen, and
  // with motion allowed it plays, muted and looping, while on screen —
  // without controls. Under reduced motion it keeps its controls and waits.
  var clips = Array.prototype.slice.call(document.querySelectorAll("video[data-auto]"));
  function poster(v) { var p = v.getAttribute("data-poster"); if (p && !v.getAttribute("poster")) v.setAttribute("poster", p); }
  function play(v) { var p = v.play(); if (p && p.catch) p.catch(function () { v.controls = true; }); }
  clips.forEach(function (v) {
    near(v, function (on) { if (on) poster(v); }, "600px 0px");
    if (reduce.matches) return;
    v.controls = false;
    near(v, function (on) { v.inView = on; if (on && !document.hidden) play(v); else v.pause(); }, "0px 0px -15% 0px");
  });

  // (11) the tour ----------------------------------------------------------------
  // The chapters are links to the clips (what a reader without JavaScript
  // follows); here a chapter loads its clip into the stage. With motion
  // allowed the stage plays while on screen and moves on to the next chapter
  // when a clip ends, back to the first after the last.
  var stageV = document.getElementById("tour-video");
  var chapters = Array.prototype.slice.call(document.querySelectorAll(".chapters a[data-clip]"));
  var title = document.getElementById("tour-title"), tcap = document.getElementById("tour-cap");
  var current = 0, tourOn = false;
  function load(i, autoplay) {
    var a = chapters[i], name = a.getAttribute("data-clip");
    current = i;
    chapters.forEach(function (c, j) { c.setAttribute("aria-current", j === i ? "true" : "false"); });
    stageV.setAttribute("poster", "media/" + name + ".poster.webp");
    stageV.setAttribute("width", a.getAttribute("data-w")); stageV.setAttribute("height", a.getAttribute("data-h"));
    stageV.querySelectorAll("source").forEach(function (src) { src.src = /webm/.test(src.type) ? "media/" + name + ".webm" : "media/" + name + ".mp4"; });
    stageV.load();
    if (title) title.textContent = "tour · " + (i + 1) + " of " + chapters.length + " · " + a.querySelector("b").textContent.toLowerCase();
    if (tcap) tcap.innerHTML = a.getAttribute("data-cap");
    if (autoplay) play(stageV);
  }
  if (stageV && chapters.length) {
    if (title) title.textContent = "tour · 1 of " + chapters.length + " · " + chapters[0].querySelector("b").textContent.toLowerCase();
    chapters.forEach(function (a, i) { a.addEventListener("click", function (ev) { ev.preventDefault(); load(i, true); }); });
    if (!reduce.matches) {
      stageV.controls = false;
      stageV.addEventListener("click", function () { if (stageV.paused) play(stageV); else stageV.pause(); });
      stageV.addEventListener("ended", function () { load((current + 1) % chapters.length, tourOn); });
      near(stageV, function (on) { stageV.inView = on; tourOn = on && !document.hidden; if (tourOn) play(stageV); else stageV.pause(); }, "0px 0px -20% 0px");
    }
  }
  // a hidden tab plays nothing; back in view, what is on screen resumes
  document.addEventListener("visibilitychange", function () {
    var all = clips.concat(stageV ? [stageV] : []);
    if (document.hidden) { tourOn = false; all.forEach(function (v) { v.pause(); }); return; }
    if (reduce.matches) return;
    all.forEach(function (v) { if (v.inView) play(v); });
    tourOn = !!(stageV && stageV.inView);
  });
})();
