'use strict';
'require view';
'require rpc';
'require poll';

// discowall: what each interface has announced — the host table and the service instances
// (`discoctl hosts`, `discoctl services`), polled every 5 s. Read-only.

const callHosts = rpc.declare({ object: 'luci.discowall', method: 'hosts' });
const callServices = rpc.declare({ object: 'luci.discowall', method: 'services' });

function titles(names) {
	return E('tr', { 'class': 'tr table-titles' }, names.map(n => E('th', { 'class': 'th' }, n)));
}

return view.extend({
	load: function() {
		return Promise.all([ callHosts(), callServices() ]);
	},

	update: function(node, data) {
		const hosts = data[0] || {};
		const services = data[1] || {};
		const err = node.querySelector('#discowall-error');
		const ok = hosts.result == 'hosts';

		err.style.display = ok ? 'none' : '';
		err.textContent = ok ? '' : _('The daemon does not answer: %s').format(hosts.message || '?');

		cbi_update_table(node.querySelector('#discowall-hosts'),
			(hosts.hosts || []).map(h => [
				h.iface, h.host, (h.addresses || []).join(' '),
				'%ds'.format(h.last_seen_s), '%ds'.format(h.expires_in_s),
				(h.instances || []).map(i => i.port != null
					? '%s (%s:%d)'.format(i.service, i.instance, i.port)
					: '%s (%s)'.format(i.service, i.instance)).join(', ')
			]),
			E('em', _('No hosts seen yet')));

		cbi_update_table(node.querySelector('#discowall-services'),
			(services.services || []).map(s => [
				s.iface, s.service, s.instance, s.host || '-',
				s.port != null ? s.port : '-', '%ds'.format(s.expires_in_s)
			]),
			E('em', _('No service instances seen yet')));
	},

	render: function(data) {
		const node = E('div', { 'class': 'cbi-map' }, [
			E('h2', _('discowall: hosts and services')),
			E('div', { 'class': 'cbi-map-descr' },
				_('Learned from every packet, forwarded or not; entries expire with their records.')),
			E('div', { 'class': 'alert-message warning', 'id': 'discowall-error', 'style': 'display:none' }),
			E('div', { 'class': 'cbi-section' }, [
				E('h3', _('Hosts')),
				E('table', { 'class': 'table', 'id': 'discowall-hosts' }, [
					titles([ _('Interface'), _('Host'), _('Addresses'), _('Last seen'), _('Expires in'), _('Services') ])
				])
			]),
			E('div', { 'class': 'cbi-section' }, [
				E('h3', _('Services')),
				E('table', { 'class': 'table', 'id': 'discowall-services' }, [
					titles([ _('Interface'), _('Service'), _('Instance'), _('Host'), _('Port'), _('Expires in') ])
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
