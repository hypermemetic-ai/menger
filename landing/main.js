// menger.sh — DESIGN-48 (DESIGN-45, DESIGN-37, MENGER-10 beneath). Progressive
// enhancement only: the page reads whole without this file. It (2) runs the boot,
// (3) types the install line as it comes into view, (4) reveals sections on
// view and develops pictures as they arrive, (5) types the terminal, (7) adds
// copy buttons to commands. (1, the Log in links, and 6, the visitor's OS
// first in Download, went with DESIGN-51; the tag landing-full-2026-10-15
// has them.) Shared by the landing page and guide/. It requests
// nothing but the page's own media. No inline style: every value it sets
// goes through the CSSOM (the CSP allows no attribute). (The carpet wall of
// DESIGN-37–45 lived here as section 3; DESIGN-48 took it off the hero.)
(function () {
  "use strict";
  var root = document.documentElement;
  root.classList.add("js");

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

  // (3) the install line types itself as it comes into view -----------------
  var typed = document.getElementById("typed");
  if (typed && !reduce.matches) {
    var line = typed.getAttribute("data-line") || typed.textContent, i = 0;
    typed.textContent = ""; typed.classList.add("caret");
    once(typed, function () {
      setTimeout(function step() { typed.textContent = line.slice(0, ++i); if (i < line.length) setTimeout(step, 28 + Math.random() * 40); else setTimeout(function () { typed.classList.remove("caret"); }, 1800); }, 300);
    }, "0px", 0.5);
  }

  // (4) reveal on view; pictures develop as they arrive ----------------------
  var reveals = document.querySelectorAll(".reveal, .develop");
  if ("IntersectionObserver" in window && !reduce.matches) {
    var ro = new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); ro.unobserve(e.target); } }); }, { rootMargin: "0px 0px -8% 0px" });
    reveals.forEach(function (el) { ro.observe(el); });
  } else {
    reveals.forEach(function (el) { el.classList.add("in"); });
  }

  // (5) the terminal types as it comes into view ------------------------------
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

  // (7) copy buttons ----------------------------------------------------------
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

  // The clip is an animated image (DESIGN-46): it plays by itself and stops
  // under reduced motion through its <picture>; nothing here touches it.
})();
