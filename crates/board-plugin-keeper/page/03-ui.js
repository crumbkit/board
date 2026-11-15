// ---- page: reads the embedded data, renders the route in the address, and re-renders when the route changes.
if (typeof document !== 'undefined') (function () {
  const read = id => JSON.parse(document.getElementById(id).textContent);
  const reversed = /[?&]order=reversed/.test(location.search);
  const data = reversed ? reverseDeep(read('data')) : read('data'), systems = reversed ? reverseDeep(read('systems')) : read('systems'), questions = reversed ? reverseDeep(read('questions')) : read('questions');
  const model = buildModel(data, systems, questions);
  const $ = id => document.getElementById(id), root = document.documentElement, dark = matchMedia('(prefers-color-scheme: dark)');
  let theme = 'auto', path = null;
  const paint = () => { root.dataset.theme = theme === 'auto' ? (dark.matches ? 'dark' : 'light') : theme; $('theme').textContent = 'Theme: ' + theme[0].toUpperCase() + theme.slice(1); };
  $('theme').addEventListener('click', () => { theme = { auto: 'light', light: 'dark', dark: 'auto' }[theme]; paint(); });
  dark.addEventListener('change', paint);
  const show = () => {
    const route = parseRoute(location.hash), out = render(model, route), here = route.page + '/' + route.id + '/' + route.view + '/' + route.f, y = window.scrollY;
    $('crumbs').innerHTML = out.crumbs; $('main').innerHTML = out.main; $('panel').innerHTML = out.panel;
    document.body.classList.toggle('has-panel', !!out.panel); document.title = out.title + ' · Work';
    if (here !== path) { window.scrollTo(0, 0); path = here; } else window.scrollTo(0, y);
    if (out.panel) $('panel').scrollTop = 0;
  };
  addEventListener('hashchange', show);
  addEventListener('keydown', e => { if (e.key === 'Escape') { const x = document.querySelector('#panel .x'); if (x) location.hash = x.getAttribute('href'); } });
  paint(); show();
  // For the checks: the routes, and the position of every keyed element on the page.
  window.workboard = { routes: () => allRoutes(model), html: () => $('crumbs').innerHTML + $('main').innerHTML + $('panel').innerHTML,
    boxes: () => JSON.stringify(Array.from(document.querySelectorAll('[data-k], h1, h2, h3, .thead > *, .f, .tag, .wait')).map(e => { const b = e.getBoundingClientRect(); return [e.dataset.k || e.tagName, b.x, b.y, b.width, b.height]; })) };
})();
if (typeof module !== 'undefined') module.exports = { buildModel, reverseDeep, render, parseRoute, allRoutes };
