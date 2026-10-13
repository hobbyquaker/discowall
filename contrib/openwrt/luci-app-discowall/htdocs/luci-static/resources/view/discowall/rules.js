'use strict';
'require view';
'require rpc';
'require ui';

// discowall: the rule chain, edited live (D-13). Every change goes to the daemon at once, as
// `discoctl rule add|insert|del` does; "Save" writes /etc/discowall/rules.toml (`discoctl save`),
// "Apply" (`discoctl apply`) throws the live chain away and reads the file again. The
// banner shows when the live chain differs from the file.

const callRules = rpc.declare({ object: 'luci.discowall', method: 'rules' });
const callStatus = rpc.declare({ object: 'luci.discowall', method: 'status' });
const callRuleAdd = rpc.declare({ object: 'luci.discowall', method: 'rule_add', params: [ 'rule', 'at' ] });
const callRuleDel = rpc.declare({ object: 'luci.discowall', method: 'rule_del', params: [ 'index' ] });
const callSave = rpc.declare({ object: 'luci.discowall', method: 'save' });
const callApply = rpc.declare({ object: 'luci.discowall', method: 'apply' });

const KINDS = [ 'query', 'probe', 'response', 'announce' ];
const PROTOS = [ 'mdns', 'ssdp' ];
const LISTS = [ 'src', 'service', 'instance', 'host', 'rrtype' ];

function quote(s) {
	return (s === '' || /[ ,"]/.test(s)) ? JSON.stringify(s) : s;
}

// The command-line form of a rule, as `discoctl rule list` prints it.
function ruleText(r) {
	const parts = [];
	if (r.proto && r.proto.length)
		parts.push('proto ' + r.proto.join(','));
	if (r.in)
		parts.push('in ' + r.in);
	if (r.out)
		parts.push('out ' + r.out);
	if (r.src && r.src.length)
		parts.push('src ' + r.src.join(','));
	if (r.kind && r.kind.length)
		parts.push('kind ' + r.kind.join(','));
	for (const k of [ 'service', 'instance', 'host', 'rrtype' ])
		if (r[k] && r[k].length)
			parts.push(k + ' ' + r[k].map(quote).join(','));
	return parts.join(' ') || _('everything');
}

function splitList(s) {
	return s.split(',').map(x => x.trim()).filter(x => x.length);
}

// A failed call comes back as {result: 'error', message} (discoctl's answer) or as a rejected
// promise (rpc/ACL); both end up here.
function failed(res) {
	if (res && res.result == 'error') {
		ui.addNotification(null, E('p', _('discowall: %s').format(res.message)), 'danger');
		return true;
	}
	return false;
}

function rejected(err) {
	ui.addNotification(null, E('p', _('discowall: %s').format(err.message || err)), 'danger');
}

return view.extend({
	load: function() {
		return Promise.all([ callRules(), callStatus() ]);
	},

	refresh: function() {
		return this.load().then(data => {
			const fresh = this.renderChain(data);
			const old = document.getElementById('discowall-chain');
			old.parentNode.replaceChild(fresh, old);
		});
	},

	interfaces: [],

	// The modal form with the RuleSpec's fields; `at` is the position to insert before (0: append).
	openForm: function(at) {
		const ifaceSelect = (id) => E('select', { 'id': id, 'class': 'cbi-input-select' },
			[ E('option', { 'value': '' }, _('any')) ].concat(
				this.interfaces.map(n => E('option', { 'value': n }, n))));
		const checks = (name, values) => E('div', {}, values.map(v =>
			E('label', { 'style': 'margin-right:1em' }, [
				E('input', { 'type': 'checkbox', 'name': name, 'value': v }), ' ', v
			])));
		const text = (id, placeholder) => E('input', {
			'id': id, 'type': 'text', 'class': 'cbi-input-text', 'placeholder': placeholder
		});
		const row = (label, field, hint) => E('div', { 'class': 'cbi-value' }, [
			E('label', { 'class': 'cbi-value-title' }, label),
			E('div', { 'class': 'cbi-value-field' }, [
				field, hint ? E('div', { 'class': 'cbi-value-description' }, hint) : ''
			])
		]);

		const form = E('div', { 'id': 'discowall-form' }, [
			row(_('Protocol'), checks('proto', PROTOS), _('None checked: both.')),
			row(_('Arriving interface'), ifaceSelect('dw-in')),
			row(_('Target interface'), ifaceSelect('dw-out')),
			row(_('Source'), text('dw-src', '192.168.30.0/24, fd00::/8'), _('Addresses or prefixes, comma-separated.')),
			row(_('Kind'), checks('kind', KINDS), _('None checked: every kind.')),
			row(_('Service'), text('dw-service', '_hap._tcp, _ipp._tcp'), _('Service types or SSDP types; globs; comma-separated.')),
			row(_('Instance'), text('dw-instance', 'living room*'), _('Instance names without the service tail; globs.')),
			row(_('Host'), text('dw-host', 'bridge, printer*'), _('Host names without .local; globs.')),
			row(_('Record type'), text('dw-rrtype', 'A, AAAA, PTR'), _('For a query, the question\'s type.')),
			row(_('Action'), E('select', { 'id': 'dw-action', 'class': 'cbi-input-select' }, [
				E('option', { 'value': 'accept' }, _('accept')),
				E('option', { 'value': 'drop' }, _('drop'))
			])),
			row(_('Log'), E('input', { 'id': 'dw-log', 'type': 'checkbox' }), _('Log every hit.')),
			row(_('Comment'), text('dw-comment', '')),
			row(_('Position'), E('input', {
				'id': 'dw-at', 'type': 'number', 'min': '1', 'class': 'cbi-input-text',
				'value': at > 0 ? String(at) : '', 'placeholder': _('end of the chain')
			}), _('Insert before this rule; empty appends.'))
		]);

		ui.showModal(at > 0 ? _('Insert rule') : _('Add rule'), [
			form,
			E('div', { 'class': 'right' }, [
				E('button', { 'class': 'btn', 'click': ui.hideModal }, _('Cancel')), ' ',
				E('button', { 'class': 'btn cbi-button-positive', 'click': ui.createHandlerFn(this, 'submitForm', form) }, _('Add'))
			])
		]);
	},

	submitForm: function(form) {
		const val = id => form.querySelector('#' + id).value.trim();
		const checked = name => Array.prototype.map.call(
			form.querySelectorAll('input[name="' + name + '"]:checked'), e => e.value);
		const rule = { action: val('dw-action') };

		if (checked('proto').length)
			rule.proto = checked('proto');
		if (val('dw-in'))
			rule.in = val('dw-in');
		if (val('dw-out'))
			rule.out = val('dw-out');
		if (checked('kind').length)
			rule.kind = checked('kind');
		for (const k of LISTS) {
			const l = splitList(val('dw-' + k));
			if (l.length)
				rule[k] = l;
		}
		if (form.querySelector('#dw-log').checked)
			rule.log = true;
		if (val('dw-comment'))
			rule.comment = val('dw-comment');

		const at = parseInt(val('dw-at'), 10) || 0;
		return callRuleAdd(rule, at).then(res => {
			if (failed(res))
				return;
			ui.hideModal();
			return this.refresh();
		}).catch(rejected);
	},

	handleDelete: function(index) {
		if (!confirm(_('Delete rule %d?').format(index)))
			return;
		return callRuleDel(index).then(res => {
			if (!failed(res))
				return this.refresh();
		}).catch(rejected);
	},

	handleSaveRules: function() {
		return callSave().then(res => {
			if (failed(res))
				return;
			ui.addNotification(null, E('p', _('The rule chain was written to the rules file.')), 'info');
			return this.refresh();
		}).catch(rejected);
	},

	handleApplyRules: function() {
		if (!confirm(_('Throw the live chain away and load the rules file again?')))
			return;
		return callApply().then(res => {
			if (!failed(res))
				return this.refresh();
		}).catch(rejected);
	},

	renderChain: function(data) {
		const rules = data[0] || {};
		const status = data[1] || {};

		if (status.result == 'status')
			this.interfaces = (status.interfaces || []).map(i => i.name);

		if (rules.result != 'rules')
			return E('div', { 'id': 'discowall-chain', 'class': 'alert-message warning' },
				_('The daemon does not answer: %s').format(rules.message || '?'));

		const table = E('table', { 'class': 'table' }, [
			E('tr', { 'class': 'tr table-titles' }, [
				E('th', { 'class': 'th' }, '#'),
				E('th', { 'class': 'th' }, _('Hits')),
				E('th', { 'class': 'th' }, _('Match')),
				E('th', { 'class': 'th' }, _('Action')),
				E('th', { 'class': 'th' }, _('Comment')),
				E('th', { 'class': 'th cbi-section-actions' }, '')
			])
		]);

		cbi_update_table(table, (rules.rules || []).map(r => [
			r.index,
			r.hits,
			E('code', ruleText(r)),
			r.action + (r.log ? ' ' + _('(log)') : ''),
			r.comment || '',
			E('div', {}, [
				E('button', { 'class': 'btn cbi-button', 'click': ui.createHandlerFn(this, 'openForm', r.index) }, _('Insert above')), ' ',
				E('button', { 'class': 'btn cbi-button-remove', 'click': ui.createHandlerFn(this, 'handleDelete', r.index) }, _('Delete'))
			])
		]), E('em', _('No rules: the default policy decides everything.')));

		return E('div', { 'id': 'discowall-chain' }, [
			rules.unsaved ? E('div', { 'class': 'alert-message warning', 'id': 'discowall-unsaved' },
				_('The live chain has changes that are not in the rules file. Save writes them; they are lost when the daemon restarts.')) : '',
			table,
			E('p', {}, _('Default policy: %s').format(rules.default)),
			E('div', { 'class': 'cbi-page-actions' }, [
				E('button', { 'class': 'btn cbi-button-add', 'click': ui.createHandlerFn(this, 'openForm', 0) }, _('Add rule')), ' ',
				E('button', { 'class': 'btn cbi-button-save', 'click': ui.createHandlerFn(this, 'handleSaveRules') }, _('Save')), ' ',
				E('button', {
					'class': 'btn cbi-button-apply', 'title': _('Load the rules file again, dropping unsaved changes'),
					'click': ui.createHandlerFn(this, 'handleApplyRules')
				}, _('Apply'))
			])
		]);
	},

	render: function(data) {
		return E('div', { 'class': 'cbi-map' }, [
			E('h2', _('discowall: rules')),
			E('div', { 'class': 'cbi-map-descr' },
				_('First match wins; a match left out matches everything. Changes take effect at once and are kept over a restart only after Save.')),
			E('div', { 'class': 'cbi-section' }, [ this.renderChain(data) ])
		]);
	},

	handleSave: null,
	handleSaveApply: null,
	handleReset: null
});
