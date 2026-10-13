'use strict';
'require view';
'require form';
'require tools.widgets as widgets';

// discowall: the daemon's settings in UCI (/etc/config/discowall, D-10). Save & Apply commits them;
// the init script renders /etc/discowall/discowall.toml and procd restarts the daemon when the
// rendered file changed. The rule chain is not here: see the Rules page.

return view.extend({
	render: function() {
		let m, s, o;

		m = new form.Map('discowall', _('discowall: settings'),
			_('The interfaces to forward mDNS and SSDP between, and the address families. Applying restarts the daemon if anything changed; unsaved rules are lost then, so save them on the Rules page first.'));

		s = m.section(form.NamedSection, 'main', 'discowall', _('Daemon'));
		s.addremove = false;

		o = s.option(widgets.DeviceSelect, 'interface', _('Interfaces'),
			_('At least two. The rules name the interfaces by these device names.'));
		o.multiple = true;
		o.noaliases = true;
		o.nocreate = true;
		o.rmempty = false;
		o.validate = function(section_id, value) {
			const list = this.formvalue(section_id);
			if (Array.isArray(list) && list.length == 1)
				return _('Name at least two interfaces.');
			return true;
		};

		o = s.option(form.Flag, 'ipv4', _('IPv4'), _('Forward over IPv4 (224.0.0.251, 239.255.255.250).'));
		o.default = '1';
		o.rmempty = false;

		o = s.option(form.Flag, 'ipv6', _('IPv6'), _('Forward over IPv6 (ff02::fb, ff02::c).'));
		o.default = '1';
		o.rmempty = false;

		o = s.option(form.Flag, 'ssdp', _('SSDP'), _('Also forward SSDP (UPnP) under the same rules.'));
		o.default = '1';
		o.rmempty = false;

		o = s.option(form.Value, 'rules_file', _('Rules file'),
			_('Where Save on the Rules page writes the chain and where the daemon reads it on start.'));
		o.placeholder = '/etc/discowall/rules.toml';
		o.rmempty = true;

		return m.render();
	}
});
