// Route check. It reads the three data blocks from fixture/page.html, which a Rust test keeps equal to what keeper.render
// writes for the synthetic fixture. It runs the page's own model and view in Node for every route, with the data as given,
// parsed again, and with every list and key order reversed, and compares the HTML text. It checks that each realm's total is
// the sum of its rows and its line of questions tied to no project, that every filter, fold and page is among the routes, and
// that no page prints a value that failed to resolve. It needs no browser.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const code = ['01-model.js', '02-view.js', '03-ui.js'].map(name => readFileSync(join(here, name), 'utf8')).join('\n');
const context = { module: { exports: {} }, console };
vm.createContext(context);
vm.runInContext(code, context);
const { buildModel, reverseDeep, render, parseRoute, allRoutes } = context.module.exports;

const page = readFileSync(join(here, 'fixture', 'page.html'), 'utf8');
const block = id => {
  const match = page.match(new RegExp(`<script type="application/json" id="${id}">\\n([^\\n]*)\\n</script>`));
  if (!match) throw new Error(`fixture/page.html has no ${id} block`);
  return JSON.parse(match[1].replaceAll('<\\/', '</'));
};
const [data, systems, questions] = ['data', 'systems', 'questions'].map(block);

const pages = (d, s, q) => {
  const model = buildModel(d, s, q);
  return allRoutes(model).map(route => {
    const out = render(model, parseRoute(route));
    return `${route}\n${out.title}\n${out.crumbs}\n${out.main}\n${out.panel}`;
  }).join('\n');
};
const clone = x => JSON.parse(JSON.stringify(x));

const problems = [];
const check = (ok, message) => { if (!ok) problems.push(message); };

const given = pages(data, systems, questions);
check(given === pages(clone(data), clone(systems), clone(questions)), 'a second render of the same data differs');
check(given === pages(reverseDeep(data), reverseDeep(systems), reverseDeep(questions)), 'reversed input renders different HTML');

const model = buildModel(data, systems, questions);
const routes = allRoutes(model);
check(routes.length === new Set(routes).size, 'a route is listed twice');
check(model.realms.length > 0 && model.projects.length > 0 && model.changes.length > 0, 'the fixture has no realms, projects or changes');
for (const realm of model.realms) check(given.includes(`sel=realm:${realm.id}`), `realm ${realm.id} has no page`);
for (const project of model.projects) check(given.includes(`#/project/${project.id}"`), `project ${project.id} has no page`);
for (const change of model.changes) check(given.includes(`sel=change:${change.id}"`), `change ${change.id} has no panel`);
for (const system of model.systems) check(given.includes(`#/system/${system.id}"`), `system ${system.id} has no page`);

// Every realm's total is the sum of its rows and its line of questions tied to no project, and the grand total is the sum of the realms.
for (const realm of model.realms) {
  const rows = realm.projects.reduce((total, id) => total + model.P[id].waiting, 0);
  check(realm.waiting === rows + realm.unassigned.length, `realm ${realm.id}: ${realm.waiting} is not ${rows} on rows and ${realm.unassigned.length} tied to no project`);
}
const asked = Object.values(questions.projects).reduce((t, list) => t + list.length, 0) + Object.values(questions.unassigned).reduce((t, list) => t + list.length, 0);
check(model.waiting === asked, `the page counts ${model.waiting} waiting questions and the data holds ${asked}`);

// The filters, each realm folded and each quiet line opened are routes.
const views = ['#/?f=owner', '#/?f=flight', '#/?f=merged', '#/?f=stale', '#/?f=tidy'].concat(model.realms.flatMap(r => [`#/?shut=${r.id}`, `#/?quiet=${r.id}`]));
for (const view of views) check(routes.includes(view), `route ${view} is not listed`);

// The header says how many open changes the map lacks.
check((data.missing > 0) === given.includes('not in the map of'), 'the header sentence about missing changes does not match data.missing');

// A value that failed to resolve prints as one of these.
for (const bad of ['undefined', 'NaN', '[object', 'null</']) check(!given.includes(bad), `a page prints ${bad}`);

if (problems.length) {
  console.error(problems.map(problem => `FAIL: ${problem}`).join('\n'));
  process.exit(1);
}
console.log(`${routes.length} routes, ${given.length} bytes of HTML; ${model.realms.length} realms, ${model.projects.length} projects, ${model.changes.length} changes, ${model.systems.length} systems; reversed input and a second render give identical HTML; every realm total is its rows plus its line of questions tied to no project.`);
