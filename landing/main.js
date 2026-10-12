// menger.sh — MENGER-7, MENGER-10. Progressive enhancement only: the page
// reads whole without this file. It (1) points the Log in links at the app
// named by <html data-app-url>, (2) animates the Menger carpet (principle 1),
// (3) reveals sections on scroll, (4) offers the visitor's OS first in the
// Download section, (5) adds copy buttons to commands, (6) plays the clips
// beside the principles while they are on screen, (7) runs the tour's
// chapters. Shared by the landing page and guide/. It requests nothing but
// the page's own media, and only as it nears the screen.
(function () {
  "use strict";
  var root = document.documentElement;
  root.classList.add("js");

  // (1) the hosted app, set in one place
  var APP_URL = (root.getAttribute("data-app-url") || "https://app.menger.sh").replace(/\/+$/, "");
  document.querySelectorAll("a[data-login]").forEach(function (a) { a.href = APP_URL + "/login"; });

  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)");

  // (2) the carpet ------------------------------------------------------
  var SVGNS = "http://www.w3.org/2000/svg";
  var svg = document.getElementById("carpet");
  var stage = document.querySelector(".sponge");
  var counts = Array.prototype.slice.call(document.querySelectorAll(".legend b[data-count]"));

  // the squares of level k (side 27 / 3^k): a square is in the carpet when
  // no base-3 digit pair of its position is (1, 1)
  function inCarpet(x, y) {
    while (x > 0 || y > 0) {
      if (x % 3 === 1 && y % 3 === 1) return false;
      x = Math.floor(x / 3); y = Math.floor(y / 3);
    }
    return true;
  }
  function squares(level) {
    var n = Math.pow(3, level), size = 27 / n, out = [];
    for (var y = 0; y < n; y++) for (var x = 0; x < n; x++) {
      if (inCarpet(x, y)) out.push({ x: x * size, y: y * size, s: size, gx: x, gy: y });
    }
    return out;
  }
  // the order work reaches the leaves: level-1 block by block (clockwise
  // from the top left), and inside a block its level-2 squares likewise,
  // with a little jitter so it reads as work, not as a scan
  var BLOCK = { "0,0": 0, "1,0": 1, "2,0": 2, "2,1": 3, "2,2": 4, "1,2": 5, "0,2": 6, "0,1": 7 };
  function rankOf(sq) {
    var b1 = BLOCK[Math.floor(sq.gx / 9) + "," + Math.floor(sq.gy / 9)];
    var b2 = BLOCK[(Math.floor(sq.gx / 3) % 3) + "," + (Math.floor(sq.gy / 3) % 3)];
    var b3 = BLOCK[(sq.gx % 3) + "," + (sq.gy % 3)];
    return b1 * 64 + b2 * 8 + b3;
  }

  var GAP = 0.09; // a hairline between squares, in grid units per side
  function rect(sq, cls) {
    var r = document.createElementNS(SVGNS, "rect");
    var g = Math.min(GAP * sq.s, 0.6); // larger squares, proportionally larger gaps
    r.setAttribute("x", (sq.x + g / 2).toFixed(3));
    r.setAttribute("y", (sq.y + g / 2).toFixed(3));
    r.setAttribute("width", (sq.s - g).toFixed(3));
    r.setAttribute("height", (sq.s - g).toFixed(3));
    r.setAttribute("rx", (sq.s * 0.12).toFixed(3));
    r.setAttribute("class", "sq " + cls);
    return r;
  }

  var leaves = squares(3).map(function (sq) { sq.rank = rankOf(sq); sq.state = 0; return sq; });
  leaves.sort(function (a, b) { return a.rank - b.rank; });

  var timers = [], tick = null, running = false, visible = true, phase = "idle";
  var layers = [];
  function later(fn, ms) { var t = setTimeout(fn, ms); timers.push(t); return t; }
  function clearTimers() { timers.forEach(clearTimeout); timers = []; if (tick) { clearInterval(tick); tick = null; } }

  function clear() {
    while (svg.firstChild) svg.removeChild(svg.firstChild);
    layers = [];
  }
  function layer(level, cls, stagger) {
    var g = document.createElementNS(SVGNS, "g");
    g.setAttribute("class", "fade");
    var list = level === 3 ? leaves : squares(level);
    var rects = list.map(function (sq, i) {
      var r = rect(sq, typeof cls === "function" ? cls(sq, i) : cls);
      if (stagger) {
        r.classList.add("enter");
        r.style.setProperty("--d", Math.round(stagger(sq, i)) + "ms");
      }
      g.appendChild(r);
      return r;
    });
    svg.appendChild(g);
    layers.push(g);
    if (stagger) {
      // two frames, so the entering state is painted before it is left
      requestAnimationFrame(function () { requestAnimationFrame(function () {
        rects.forEach(function (r) { r.classList.remove("enter"); });
      }); });
    }
    return { g: g, rects: rects };
  }
  function retire(g) {
    g.classList.add("gone");
    later(function () { if (g.parentNode) g.parentNode.removeChild(g); }, 800);
  }

  function setCounts() {
    var c = [0, 0, 0, 0, 0];
    leaves.forEach(function (l) { c[l.state]++; });
    counts.forEach(function (b, i) { b.textContent = phase === "work" || phase === "done" || phase === "still" ? String(c[i]) : ""; });
  }

  // one cycle: a scope (level 0) divides into 8, 64, 512 leaves; the leaves
  // are Pending, then the work moves them through Ready, Coding and In
  // Review to Complete — serial at the smallest level, parallel above it,
  // ever more at once (work(), below); held a moment, the cycle begins again
  var leafRects = null;
  function cycle() {
    clear();
    svg.classList.remove("gone");
    leaves.forEach(function (l) { l.state = 0; });
    phase = "divide"; setCounts();
    var l0 = layer(0, "s-scope", function () { return 0; });
    later(function () {
      var l1 = layer(1, "s-scope", function (sq, i) { return i * 45; });
      retire(l0.g);
      later(function () {
        var l2 = layer(2, "s-scope", function (sq, i) { return (sq.gx + sq.gy) * 28; });
        retire(l1.g);
        later(function () {
          var l3 = layer(3, "s-0", function (sq) { return (sq.gx + sq.gy) * 14; });
          leafRects = l3.rects;
          retire(l2.g);
          later(work, 1500);
        }, 1500);
      }, 1300);
    }, 1100);
  }
  // The work. The 512 leaves are 64 chains of 8: a level-2 block's leaves
  // are worked one after another — serial at the smallest level — while the
  // chains run side by side, parallel at every level above. Every level-1
  // block starts a chain before any starts a second, the starts come ever
  // faster and each step is ever quicker, so the concurrency accelerates.
  // Leaves are sorted by rank (b1·64 + b2·8 + b3): chain b1·8 + b2 is the
  // leaves at 8·(b1·8 + b2) … +7. A chain's place is its first unfinished
  // leaf, so a paused cycle resumes where it stopped.
  var STEP_SLOW = 240, STEP_FAST = 40, START_GAP = 700, GAP_MIN = 25, GAP_SHRINK = 0.8;
  function work() {
    phase = "work"; setCounts();
    var total = leaves.length * 4, steps = 0;
    leaves.forEach(function (l) { steps += l.state; });
    var order = [];
    for (var b2 = 0; b2 < 8; b2++) for (var b1 = 0; b1 < 8; b1++) {
      var at = (b1 * 8 + b2) * 8;
      if (leaves[at + 7].state < 4) order.push({ at: at });
    }
    var now = Date.now(), gap = START_GAP;
    order.forEach(function (c, i) { c.next = now; now += gap; gap = Math.max(GAP_MIN, gap * GAP_SHRINK); });
    tick = setInterval(function () {
      var t = Date.now(), moved = false;
      var left = 1 - steps / total;
      var step = STEP_FAST + (STEP_SLOW - STEP_FAST) * left * left;
      order.forEach(function (c) {
        if (t < c.next) return;
        var j = c.at;
        while (j < c.at + 8 && leaves[j].state === 4) j++;
        if (j === c.at + 8) return;
        leaves[j].state++;
        leafRects[j].setAttribute("class", "sq s-" + leaves[j].state);
        steps++; moved = true;
        c.next = t + step * (0.7 + Math.random() * 0.6);
      });
      if (moved) setCounts();
      if (steps >= total) {
        clearInterval(tick); tick = null;
        phase = "done";
        later(function () { svg.classList.add("fade"); svg.classList.add("gone"); }, 2600);
        later(function () { svg.classList.remove("gone"); cycle(); }, 3400);
      }
    }, 25);
  }

  // reduced motion: one still frame — the carpet mid-work, every state shown
  function still() {
    clearTimers();
    clear();
    phase = "still";
    var bands = [0.38, 0.52, 0.64, 0.78]; // complete, in review, coding, ready; the rest pending
    leaves.forEach(function (l, i) {
      var f = i / leaves.length;
      l.state = f < bands[0] ? 4 : f < bands[1] ? 3 : f < bands[2] ? 2 : f < bands[3] ? 1 : 0;
    });
    layer(3, function (sq) { return "s-" + sq.state; });
    svg.classList.remove("gone");
    setCounts();
  }

  function start() {
    if (running || reduce.matches) return;
    running = true;
    stage.classList.remove("paused");
    if (phase === "work" && leafRects) work(); // resume where it stopped
    else cycle();
  }
  function stop() {
    if (!running) return;
    running = false;
    stage.classList.add("paused");
    clearTimers();
    if (phase === "divide") phase = "idle"; // a half-divided carpet restarts its cycle
  }
  function sync() {
    if (reduce.matches) { stop(); still(); return; }
    if (visible && !document.hidden) start(); else stop();
  }

  if (svg && stage) {
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (es) {
        visible = es[es.length - 1].isIntersecting;
        sync();
      }, { threshold: 0.05 }).observe(stage);
    }
    document.addEventListener("visibilitychange", sync);
    if (reduce.addEventListener) reduce.addEventListener("change", function () { phase = "idle"; sync(); });
    if (reduce.matches) still(); else sync();
  }

  // (3) scroll reveal -----------------------------------------------------
  var reveals = document.querySelectorAll(".reveal");
  if ("IntersectionObserver" in window && !reduce.matches) {
    var ro = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); ro.unobserve(e.target); } });
    }, { rootMargin: "0px 0px -8% 0px" });
    reveals.forEach(function (el) { ro.observe(el); });
  } else {
    reveals.forEach(function (el) { el.classList.add("in"); });
  }

  // (4) the visitor's OS first in Download ------------------------------
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
    if (main) main.textContent = "Download for " + NAMES[os];
    if (hint) hint.textContent = HINTS[os];
    document.querySelectorAll("a[data-download]").forEach(function (a) { a.textContent = "Download for " + NAMES[os]; });
    if (os === "mac" && hint && navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
      navigator.userAgentData.getHighEntropyValues(["architecture"]).then(function (v) {
        if (v.architecture === "arm") hint.textContent = "Apple Silicon: the aarch64 .dmg, on the release page.";
        else if (v.architecture === "x86") hint.textContent = "Intel: the x64 .dmg, on the release page.";
      }, function () {});
    }
  } else if (os === "mobile" && hint) {
    hint.textContent = "The desktop app is for Linux, macOS and Windows; mobile apps are coming.";
  }

  // (5) copy buttons --------------------------------------------------------
  if (navigator.clipboard && window.isSecureContext) {
    document.querySelectorAll("pre.cmd").forEach(function (pre) {
      var b = document.createElement("button");
      b.type = "button"; b.className = "copy"; b.textContent = "Copy";
      b.setAttribute("aria-label", "Copy these commands");
      b.addEventListener("click", function () {
        // the commands only: no comments, no sample output
        var code = pre.querySelector("code").cloneNode(true);
        code.querySelectorAll(".o, .c").forEach(function (n) { n.remove(); });
        var text = code.textContent
          .split("\n").filter(function (l) { return l.trim() && !/^\s*#/.test(l); })
          .map(function (l) { return l.replace(/\s+#.*$/, ""); }).join("\n");
        navigator.clipboard.writeText(text).then(function () {
          b.textContent = "Copied";
          setTimeout(function () { b.textContent = "Copy"; }, 1600);
        }, function () { b.textContent = "Select to copy"; });
      });
      pre.appendChild(b);
    });
  }

  // (6) the principles' clips ------------------------------------------------
  // In the HTML each has controls and no poster (a poster is fetched at once,
  // wherever the video is): here the poster is set as it nears the screen,
  // and with motion allowed it plays, muted and looping, while on screen —
  // without controls. Under reduced motion it keeps its controls and waits.
  var clips = Array.prototype.slice.call(document.querySelectorAll("video[data-auto]"));
  function near(el, fn, margin) {
    if (!("IntersectionObserver" in window)) { fn(true); return; }
    new IntersectionObserver(function (es) {
      es.forEach(function (e) { fn(e.isIntersecting); });
    }, { rootMargin: margin || "0px" }).observe(el);
  }
  function poster(v) {
    var p = v.getAttribute("data-poster");
    if (p && !v.getAttribute("poster")) v.setAttribute("poster", p);
  }
  function play(v) {
    var p = v.play();
    if (p && p.catch) p.catch(function () { v.controls = true; });
  }
  clips.forEach(function (v) {
    near(v, function (on) { if (on) poster(v); }, "600px 0px");
    if (reduce.matches) return;
    v.controls = false;
    near(v, function (on) {
      v.inView = on;
      if (on && !document.hidden) play(v); else v.pause();
    }, "0px 0px -15% 0px");
  });

  // (7) the tour --------------------------------------------------------------
  // The chapters are links to the clips (what a reader without JavaScript
  // follows); here a chapter loads its clip into the stage. With motion
  // allowed the stage plays while on screen and moves on to the next chapter
  // when a clip ends, back to the first after the last.
  var stageV = document.getElementById("tour-video");
  var chapters = Array.prototype.slice.call(document.querySelectorAll(".chapters a[data-clip]"));
  var title = document.getElementById("tour-title"), cap = document.getElementById("tour-cap");
  var current = 0, tourOn = false;
  function load(i, autoplay) {
    var a = chapters[i], name = a.getAttribute("data-clip");
    current = i;
    chapters.forEach(function (c, j) { c.setAttribute("aria-current", j === i ? "true" : "false"); });
    stageV.setAttribute("poster", "media/" + name + ".poster.webp");
    stageV.setAttribute("width", a.getAttribute("data-w"));
    stageV.setAttribute("height", a.getAttribute("data-h"));
    stageV.querySelectorAll("source").forEach(function (src) {
      src.src = /webm/.test(src.type) ? "media/" + name + ".webm" : "media/" + name + ".mp4";
    });
    stageV.load();
    if (title) title.textContent = a.querySelector("b").textContent;
    if (cap) cap.innerHTML = a.getAttribute("data-cap");
    if (autoplay) play(stageV);
  }
  if (stageV && chapters.length) {
    chapters.forEach(function (a, i) {
      a.addEventListener("click", function (ev) {
        ev.preventDefault();
        load(i, true); // a click is the reader's own: it plays, reduced motion or not
      });
    });
    if (!reduce.matches) {
      stageV.controls = false;
      stageV.addEventListener("click", function () { if (stageV.paused) play(stageV); else stageV.pause(); });
      stageV.addEventListener("ended", function () { load((current + 1) % chapters.length, tourOn); });
      near(stageV, function (on) {
        stageV.inView = on;
        tourOn = on && !document.hidden;
        if (tourOn) play(stageV); else stageV.pause();
      }, "0px 0px -20% 0px");
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
