// Drives the LuCI pages of luci-app-discowall in a real browser (headless Firefox through
// playwright-core): login, the menu, each view, the rule editor's modal, Save, the settings form with
// Save & Apply. lab.py runs it for `run --ui openwrt` (tests/lab/openwrt.md); by hand:
//   NODE_PATH=<dir with playwright-core> FF=<firefox executable> node luci-ui.js <base URL> <password file> <screenshot dir>
// Prints the checks as JSON; exit code 0 when all pass. Headless Chromium under WSL never painted a
// frame (LuCI's views stayed at "Loading view…", screenshots timed out), Firefox did: 2026-10-13.
const { firefox } = require('playwright-core');
const fs = require('fs');
const base = process.argv[2], pw = fs.readFileSync(process.argv[3], 'utf8').trim(), out = process.argv[4];
const results = [];
let page;
const shot = async o => { try { await page.screenshot(Object.assign({ timeout: 15000, animations: 'disabled' }, o)); } catch (e) { console.error('screenshot', o.path, e.message.split('\n')[0]); } };
const check = (name, ok, detail) => { results.push({ name, ok: !!ok, detail: detail || '' }); };
(async () => {
  const browser = await firefox.launch({ executablePath: process.env.FF });
  page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message + ' @ ' + (e.stack || '').split('\n').slice(0, 6).join(' < ') + ' url=' + page.url()));
  page.on('console', m => { if (m.type() === 'error') errors.push('console: ' + m.text()); });
  await page.goto(base + '/cgi-bin/luci/');
  const pwField = page.locator('input[name=luci_password]:visible');
  await pwField.waitFor({ timeout: 60000 });
  await pwField.fill(pw);
  await Promise.all([page.waitForNavigation(), pwField.press('Enter')]);
  errors.length = 0; // the stock login page itself fails a uci/get before the login

  await page.goto(base + '/cgi-bin/luci/admin/services/discowall/status');
  await page.waitForSelector('#discowall-ifaces .tr:not(.table-titles)', { timeout: 15000 });
  await page.waitForTimeout(1500);
  const menu = await page.locator('a[href*="admin/services/discowall/"]').evaluateAll(as => as.map(a => a.textContent.trim()));
  check('menu: Services > discowall with four pages', ['Status', 'Rules', 'Hosts and services', 'Settings'].every(t => menu.includes(t)), menu.join(', '));
  const ifRows = await page.locator('#discowall-ifaces .tr:not(.table-titles)').allInnerTexts();
  check('status: interface rows', ifRows.length === 3, ifRows.join(' / '));
  check('status: version line', /discowalld/.test(await page.locator('#discowall-version').innerText()));
  await shot({ path: out + '/status.png', fullPage: true });

  await page.goto(base + '/cgi-bin/luci/admin/services/discowall/rules');
  await page.waitForSelector('#discowall-chain', { timeout: 15000 });
  const before = await page.locator('#discowall-chain .tr:not(.table-titles)').count();
  await page.click('text=Add rule');
  await page.waitForSelector('#discowall-form');
  await page.check('#discowall-form input[name=proto][value=mdns]');
  await page.selectOption('#dw-in', { index: 2 });
  await page.selectOption('#dw-out', { index: 1 });
  await page.check('#discowall-form input[name=kind][value=query]');
  await page.fill('#dw-service', '_http._tcp, _ssh._tcp');
  await page.selectOption('#dw-action', 'drop');
  await page.check('#dw-log');
  await page.fill('#dw-comment', 'from the browser');
  await page.fill('#dw-at', '1');
  await shot({ path: out + '/rules-modal.png', fullPage: true });
  await page.click('.modal button.cbi-button-positive');
  await page.waitForFunction(n => document.querySelectorAll('#discowall-chain .tr:not(.table-titles)').length === n + 1, before, { timeout: 10000 });
  const first = await page.locator('#discowall-chain .tr:not(.table-titles)').first().innerText();
  check('rules: modal adds a rule at position 1', /proto mdns .*kind query service _http._tcp,_ssh._tcp/.test(first) && /drop/.test(first) && /from the browser/.test(first), first);
  check('rules: unsaved banner shown', await page.locator('#discowall-unsaved').count() === 1);
  await shot({ path: out + '/rules.png', fullPage: true });
  page.once('dialog', d => d.accept());
  await page.locator('#discowall-chain .tr:not(.table-titles)').first().locator('button.cbi-button-remove').click();
  await page.waitForFunction(n => document.querySelectorAll('#discowall-chain .tr:not(.table-titles)').length === n, before, { timeout: 10000 });
  check('rules: delete removes it', true);
  await page.click('button.cbi-button-save');
  await page.waitForFunction(() => !document.querySelector('#discowall-unsaved'), null, { timeout: 10000 });
  check('rules: Save clears the banner', true);

  await page.goto(base + '/cgi-bin/luci/admin/services/discowall/hosts');
  await page.waitForSelector('#discowall-hosts .tr:not(.table-titles)', { timeout: 15000 });
  const hosts = await page.locator('#discowall-hosts').innerText();
  check('hosts: iothost listed', /discowall-iothost/.test(hosts), hosts.slice(0, 200));
  const svcs = await page.locator('#discowall-services').innerText();
  check('services: the printer listed', /Lab Printer/i.test(svcs), svcs.slice(0, 200));
  await shot({ path: out + '/hosts.png', fullPage: true });

  await page.goto(base + '/cgi-bin/luci/admin/services/discowall/settings');
  await page.waitForSelector('.cbi-map', { timeout: 15000 });
  await page.waitForTimeout(1500);
  const settings = await page.locator('.cbi-map').innerText();
  check('settings: form renders', /Interfaces/.test(settings) && /SSDP/.test(settings), settings.slice(0, 300));
  await shot({ path: out + '/settings.png', fullPage: true });
  // Save & Apply with SSDP off: LuCI applies with rollback and confirms; the init script re-renders
  // the TOML and procd restarts the daemon. The status page then shows a fresh uptime.
  const ssdp = page.locator('#cbi-discowall-main-ssdp input[type=checkbox]');
  await ssdp.uncheck();
  await page.click('text=Save & Apply');
  await page.waitForSelector('text=Configuration changes applied', { timeout: 60000 });
  check('settings: Save & Apply (SSDP off) applied', true);
  await page.goto(base + '/cgi-bin/luci/admin/services/discowall/status');
  await page.waitForSelector('#discowall-ifaces .tr:not(.table-titles)', { timeout: 15000 });
  const ver = await page.locator('#discowall-version').innerText();
  check('settings: the daemon restarted (uptime under a minute)', /up 0h 0m \d+s/.test(ver), ver);
  await page.goto(base + '/cgi-bin/luci/admin/services/discowall/settings');
  await page.waitForSelector('#cbi-discowall-main-ssdp input[type=checkbox]', { timeout: 15000 });
  await page.locator('#cbi-discowall-main-ssdp input[type=checkbox]').check();
  await page.click('text=Save & Apply');
  await page.waitForSelector('text=Configuration changes applied', { timeout: 60000 });

  // Polls in flight when a test navigates away are aborted by the browser: not an error of the app.
  // Only errors on the app's pages count (the stock login and overview pages abort their own XHRs
  // when the test navigates on).
  const real = errors.filter(e => /url=[^ ]*discowall/.test(e) && !/XHR request aborted by browser/.test(e));
  check('no JavaScript errors', real.length === 0, errors.join(' | '));
  await browser.close();
  console.log(JSON.stringify(results, null, 1));
  process.exit(results.every(r => r.ok) ? 0 : 1);
})().catch(e => { console.log(JSON.stringify(results, null, 1)); console.error(e); process.exit(2); });
