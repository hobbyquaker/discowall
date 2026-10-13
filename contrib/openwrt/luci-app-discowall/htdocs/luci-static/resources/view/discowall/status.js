'use strict';
'require view';
'require rpc';
'require poll';

// discowall: the daemon's status — interfaces, joined groups, counters (`discoctl status`), polled
// every 5 s.

const callStatus = rpc.declare({
	object: 'luci.discowall',
	method: 'status'
});

const callStats = rpc.declare({
	object: 'luci.discowall',
	method: 'stats'
});

function joined(i) {
	return [ i.joined_v4 ? 'v4' : null, i.joined_v6 ? 'v6' : null ].filter(Boolean).join(' ') || '-';
}

function state(i) {
	if (i.index == null)
		return _('absent');
	return i.up ? _('up') : _('down');
}

const COUNTERS = [
	[ 'rx', _('Received') ],
	[ 'tx', _('Sent') ],
	[ 'dropped_by_rules', _('Dropped by the rules') ],
	[ 'records_dropped', _('Records removed') ],
	[ 'dropped_duplicate', _('Duplicates dropped') ],
	[ 'dropped_own_source', _('Own packets dropped') ],
	[ 'dropped_unparsable', _('Unparsable') ],
	[ 'dropped_unknown_interface', _('From other interfaces') ],
	[ 'dropped_link_local', _('Link-local records removed') ],
	[ 'proxied_queries', _('One-shot queries proxied') ],
	[ 'proxied_answers', _('Answers relayed') ],
	[ 'send_errors', _('Send errors') ]
];

return view.extend({
	load: function() {
		return Promise.all([ callStatus(), callStats() ]);
	},

	update: function(node, data) {
		const st = data[0] || {};
		const stats = data[1] || {};
		const err = node.querySelector('#discowall-error');
		const ok = st.result == 'status';

		err.style.display = ok ? 'none' : '';
		err.textContent = ok ? '' : _('The daemon does not answer: %s').format(st.message || '?');
		if (!ok)
			return;

		node.querySelector('#discowall-version').textContent =
			_('discowalld %s, up %s, configuration %s').format(st.version, '%t'.format(st.uptime_s), st.config_path);

		cbi_update_table(node.querySelector('#discowall-ifaces'),
			(st.interfaces || []).map(i => [
				i.name, i.index != null ? i.index : '-', state(i), joined(i),
				i.rx, i.tx, (i.addresses || []).join(' ')
			]),
			E('em', _('No interfaces configured')));

		const c = st.counters || {};
		cbi_update_table(node.querySelector('#discowall-counters'),
			COUNTERS.map(k => [ k[1], c[k[0]] != null ? c[k[0]] : '-' ]));

		if (stats.result == 'stats')
			node.querySelector('#discowall-tables').textContent =
				_('%d hosts and %d service instances known; the default policy matched %d times.')
					.format(stats.hosts, stats.instances, stats.default_hits);
	},

	render: function(data) {
		const node = E('div', { 'class': 'cbi-map' }, [
			E('h2', _('discowall')),
			E('div', { 'class': 'cbi-map-descr' },
				_('The discovery firewall forwards mDNS and SSDP between the interfaces under the rule chain.')),
			E('div', { 'class': 'alert-message warning', 'id': 'discowall-error', 'style': 'display:none' }),
			E('div', { 'class': 'cbi-section' }, [
				E('p', { 'id': 'discowall-version' }),
				E('h3', _('Interfaces')),
				E('table', { 'class': 'table', 'id': 'discowall-ifaces' }, [
					E('tr', { 'class': 'tr table-titles' }, [
						E('th', { 'class': 'th' }, _('Interface')),
						E('th', { 'class': 'th' }, _('Index')),
						E('th', { 'class': 'th' }, _('State')),
						E('th', { 'class': 'th' }, _('Joined')),
						E('th', { 'class': 'th' }, _('Received')),
						E('th', { 'class': 'th' }, _('Sent')),
						E('th', { 'class': 'th' }, _('Addresses'))
					])
				]),
				E('h3', _('Counters')),
				E('p', { 'id': 'discowall-tables' }),
				E('table', { 'class': 'table', 'id': 'discowall-counters' }, [
					E('tr', { 'class': 'tr table-titles' }, [
						E('th', { 'class': 'th' }, _('Counter')),
						E('th', { 'class': 'th' }, _('Value'))
					])
				])
			])
		]);

		this.update(node, data);
		// A failed poll (the router restarting, a request cut off) shows in the banner and the next
		// poll tries again.
		poll.add(() => this.load()
			.catch(e => [ { result: 'error', message: e.message }, {} ])
			.then(d => this.update(node, d)), 5);
		return node;
	},

	handleSave: null,
	handleSaveApply: null,
	handleReset: null
});
