const header = document.querySelector("[data-header]");
const menuButton = document.querySelector(".menu-toggle");
const nav = document.querySelector(".site-nav");

const updateHeader = () => header?.classList.toggle("scrolled", window.scrollY > 24);
updateHeader();
window.addEventListener("scroll", updateHeader, { passive: true });

menuButton?.addEventListener("click", () => {
  const open = menuButton.getAttribute("aria-expanded") !== "true";
  menuButton.setAttribute("aria-expanded", String(open));
  nav?.classList.toggle("open", open);
});

nav?.querySelectorAll("a").forEach((link) => {
  link.addEventListener("click", () => {
    nav.classList.remove("open");
    menuButton?.setAttribute("aria-expanded", "false");
  });
});

const revealTargets = document.querySelectorAll("[data-reveal]");
if ("IntersectionObserver" in window) {
  const revealObserver = new IntersectionObserver((entries, observer) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.add("revealed");
      observer.unobserve(entry.target);
    });
  }, { threshold: 0.12 });
  revealTargets.forEach((target) => revealObserver.observe(target));
} else {
  revealTargets.forEach((target) => target.classList.add("revealed"));
}

const sections = [...document.querySelectorAll("main section[id]")];
const navLinks = [...document.querySelectorAll(".site-nav a")];
if ("IntersectionObserver" in window) {
  const sectionObserver = new IntersectionObserver((entries) => {
    const visible = entries.find((entry) => entry.isIntersecting);
    if (!visible) return;
    navLinks.forEach((link) => link.classList.toggle("active", link.hash === `#${visible.target.id}`));
  }, { rootMargin: "-25% 0px -65%", threshold: 0 });
  sections.forEach((section) => sectionObserver.observe(section));
}

document.querySelectorAll("[data-code-tab]").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".code-tab").forEach((item) => {
      item.classList.toggle("active", item === tab);
      item.setAttribute("aria-selected", String(item === tab));
    });
    document.querySelectorAll("[data-code-panel]").forEach((panel) => {
      panel.classList.toggle("active", panel.dataset.codePanel === tab.dataset.codeTab);
    });
    const copyButton = document.querySelector("[data-copy-target]");
    copyButton?.setAttribute("data-copy-target", `code-${tab.dataset.codeTab}`);
  });
});

document.querySelectorAll("[data-copy-target]").forEach((button) => {
  button.addEventListener("click", async () => {
    const code = document.getElementById(button.dataset.copyTarget)?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(code);
      button.textContent = "Copiado";
    } catch {
      button.textContent = "Selecciona y copia";
    }
    window.setTimeout(() => { button.textContent = "Copiar"; }, 1800);
  });
});
