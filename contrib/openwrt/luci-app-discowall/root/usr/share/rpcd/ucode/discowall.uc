#!/usr/bin/env ucode
// rpcd plugin of luci-app-discowall: the ubus object `luci.discowall`. Every method runs
// `discoctl --json …` and returns its answer, parsed (D-13). The answers are the control protocol's
// responses ({"result": "status", …}, {"result": "error", "message": …}); when discoctl cannot reach
// the daemon it prints plain text and exits 3, which comes back as {"result": "error", "message": …,
// "exit": 3}.

'use strict';

import { popen } from 'fs';

const DISCOCTL = '/usr/bin/discoctl';

function shellquote(s) {
	return `'${replace(s, "'", "'\\''")}'`;
}

function discoctl(args) {
	const fd = popen(`${DISCOCTL} --json ${args} 2>&1`, 'r');
	if (!fd)
		return { result: 'error', message: 'cannot run discoctl' };
	const out = fd.read('all');
	const code = fd.close();
	let answer = null;
	try {
		answer = json(out);
	}
	catch (e) {
		answer = null;
	}
	if (type(answer) != 'object')
		return { result: 'error', message: trim(out) || 'no answer from discoctl', exit: code };
	return answer;
}

const methods = {
	status: { call: () => discoctl('status') },
	rules: { call: () => discoctl('rule list') },
	hosts: { call: () => discoctl('hosts') },
	services: { call: () => discoctl('services') },
	stats: { call: () => discoctl('stats') },

	// rule: a RuleSpec object; at: insert before this 1-based position, 0 or absent appends.
	rule_add: {
		args: { rule: {}, at: 0 },
		call: function(req) {
			const rule = req.args?.rule;
			if (type(rule) != 'object')
				return { result: 'error', message: 'rule must be an object' };
			const spec = shellquote(sprintf('%J', rule));
			const at = int(req.args?.at ?? 0);
			if (at > 0)
				return discoctl(`rule insert ${at} ${spec}`);
			return discoctl(`rule add ${spec}`);
		}
	},

	rule_del: {
		args: { index: 0 },
		call: function(req) {
			const index = int(req.args?.index ?? 0);
			if (index < 1)
				return { result: 'error', message: 'index must be 1 or more' };
			return discoctl(`rule del ${index}`);
		}
	},

	rule_flush: { call: () => discoctl('rule flush') },
	save: { call: () => discoctl('save') },
	apply: { call: () => discoctl('apply') }
};

return { 'luci.discowall': methods };
