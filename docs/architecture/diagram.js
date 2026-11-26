/* Mount each Mermaid diagram with Pele.
 *
 * A diagram is one element with an id, and one <pre> with the same id and "-source".
 * The source stays in the page, so a reader can use it when Pele does not load.
 */
(function () {
  "use strict";

  function fail(target, message) {
    target.innerHTML = '<p class="diagram-error">' + message + "</p>";
  }

  function mount(target) {
    var source = document.getElementById(target.id + "-source");
    if (!source) {
      fail(target, "The diagram source is missing. The page is incomplete.");
      return;
    }
    if (!window.pele || typeof window.pele.mount !== "function") {
      fail(target, "Pele did not load. The diagram source is below.");
      return;
    }
    try {
      window.pele.mount(target, source.textContent.trim());
    } catch (error) {
      console.error("Pele could not render " + target.id + ".", error);
      fail(target, "Pele could not render the diagram. The source is below.");
    }
  }

  function boot() {
    var targets = document.querySelectorAll(".diagram[id]");
    for (var i = 0; i < targets.length; i += 1) {
      mount(targets[i]);
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
}());
