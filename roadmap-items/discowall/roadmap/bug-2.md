## B-2. Dedup drops the IPv4 twin of a query avahi sends on both families at once 🏗️

**Created** 2026-10-13.

**Found** 2026-10-13 in the Proxmox lab (task 11, case 2): with real avahi on both ends, a browse from the lan host
resolved the printer over IPv6 only; `avahi-resolve-host-name -4` failed. `discoctl watch` showed why: avahi sends
the same query bytes over IPv6 and IPv4 within a millisecond, and `is_duplicate` keys on the payload alone, so the
second copy — the IPv4 one here — is dropped as a duplicate. The namespace test did not catch it because it sends
the two families half a second apart.

**Fix:** the dedup key is payload **and address family**; a unit test sends the same bytes over v4 and v6 back to
back and expects both forwarded. The second-reflector case (same bytes, same family, another source) still dedups.
