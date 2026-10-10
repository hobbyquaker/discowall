# discowall

[:uk: English README](README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!CAUTION]
> discowall ist in Planung. Es gibt noch keinen Code, nur die Recherche, die Entscheidungen und die Roadmap.

discowall ist eine **Discovery-Firewall** für einen Linux-Router: Es leitet mDNS-Pakete (Bonjour, Zeroconf,
RFC 6762) von einer Netzwerkschnittstelle auf andere weiter, so wie es der Reflector von avahi-daemon tut, aber
nach **Regeln**: welche Hosts und welche Dienste aus welchem Netz in welchem anderen Netz sichtbar werden, als
Whitelist oder Blacklist, nach Quelle, Ziel und Richtung. Eine einfache Form von iptables für `.local`. Dazu eine
CLI, die zeigt, welche Ankündigungen auf welcher Schnittstelle ankommen und welche Regel sie passieren lassen, und
die sich in Skripten benutzen lässt. Später soll SSDP (UPnP) dazukommen.

## Warum

mDNS ist auf das lokale Netzsegment beschränkt. Wer seine Geräte in VLANs trennt (IoT, Gäste, Medien), verliert
damit AirPlay, HomeKit, Chromecast, Drucker und alles andere, das sich über Bonjour findet. Der übliche Ausweg
ist avahis Reflector: Er spiegelt **alles** in **alle** Netze, und seine einzige Filtermöglichkeit ist eine flache
Liste von Diensttypen, die für alle Schnittstellen gleich gilt. Keine Richtung ("der HomeKit-Bridge aus dem
IoT-Netz darf im LAN gesehen werden, aber nichts aus dem LAN im IoT-Netz"), keine Regel nach Quelladresse, keine
Sicht darauf, was gerade warum weitergeleitet wird. Die kleineren Reflector-Werkzeuge filtern gar nicht.

discowall macht aus dem Reflector eine Firewall: eine geordnete Regelkette, erste Übereinstimmung gewinnt, eine
Default-Policy, und eine Regel für einen Host oder Dienst gilt für das ganze Bündel aus PTR-, SRV-, TXT- und
A/AAAA-Records, das ein Dienst ist.

## Was es tun soll

- **Weiterleiten** zwischen konfigurierten Schnittstellen, IPv4 und IPv6, ohne Schleifen (nie zurück auf die
  Eingangsschnittstelle, eigene Pakete verworfen, Deduplizierung gegen einen zweiten Reflector).
- **Regeln** nach Eingangs- und Ausgangsschnittstelle, ursprünglicher Quelladresse oder Präfix, Art des Pakets
  (Anfrage, Antwort, Ankündigung, Probe), Diensttyp, Instanzname, Hostname und Record-Typ; Aktion `accept` oder
  `drop`, wahlweise mit `log`.
- **Eine CLI** `discoctl`: `rule list|add|insert|del|flush`, `save`, `apply`, `test`; `watch` für den Live-Strom
  der Pakete mit Entscheidung und Regel, `hosts` und `services` für die gelernte Tabelle, `stats`, `status`. Jeder
  Befehl mit `--json`, feste Exit-Codes, nichts Interaktives.
- **Neben avahi:** discowall ist kein mDNS-Responder und kein Resolver. avahi darf auf demselben Host weiterlaufen
  und den Router als `router.local` bekannt machen; discowall übernimmt nur das Weiterleiten.
- **Einmalige Anfragen** (z. B. `dig @224.0.0.251 -p 5353`) werden stellvertretend gestellt und die Antworten per
  Unicast an den Fragenden zurückgegeben, was avahis Reflector nicht kann.
- **Ein Daemon** `discowalld`, ein statisches Binary in Rust, ohne Root nach dem Start, ohne Raw-Sockets, mit
  systemd-Unit; Pakete für Debian/Ubuntu/Armbian, Arch, Fedora und OpenWrt, später FreeBSD/pfSense.

Beispiel: HomeKit aus dem IoT-Netz im LAN sichtbar machen, sonst nichts.

```sh
discoctl default drop
discoctl rule add in lan out iot kind query    service _hap._tcp  accept   # das LAN darf im IoT-Netz nach HomeKit fragen
discoctl rule add in iot out lan kind response host bridge         accept   # nur die Antworten der Bridge kommen zurück
discoctl watch --iface iot
```

Discovery ist nicht der Datenpfad: Die Verbindung zum gefundenen Gerät (TCP 51826 für HomeKit, 631 für IPP, 7000
für AirPlay) braucht weiterhin eine Firewall-Regel. Ein späterer Ausbau soll die Adressen erlaubter Hosts in
nftables-Sets pflegen, damit genau diese Regel pro Netzpaar reicht.

## Stand

Die Recherche vom 10. Oktober 2026 (Überblick über avahi und die vorhandenen Reflectoren, Entwurf, Bewertung von
Rust, Aufwandsschätzung, Protokoll der Entscheidungen) steht in
[mdns-router-research.md](mdns-router-research.md). Die Entscheidungen sind in
[roadmap-items/discowall/decisions.md](roadmap-items/discowall/decisions.md), die Aufgaben einzeln in
[roadmap-readmes/discowall.md](roadmap-readmes/discowall.md), die offenen Fragen in
[roadmap-items/discowall/overview.md](roadmap-items/discowall/overview.md). Geschätzt sind 20 bis 30
Personentage für die erste Version.

## Roadmap

- 🕵️ **P0:** Cargo-Projekt, Sockets, Schnittstellen-Überwachung per Netlink, einfacher Reflector mit
  Schleifenschutz, systemd-Unit
- ☑️ **Lab:** Koexistenz mit avahi-daemon auf Port 5353 messen
- 🕵️ **P1:** Parser, Host-Tabelle und Record-Bündel, Regelkette, Steuer-Socket, `discoctl rule …`
- ☑️ **P2:** `discoctl watch`, `hosts`, `services`, `stats`, `status`; Zähler pro Regel, `log`
- ☑️ **P3:** Stellvertreter für einmalige Anfragen und das QU-Bit, Link-local-AAAA entfernen, IPv6 durchgehend,
  Deduplizierung, Fuzzing, Härtung der Unit
- ☑️ **P4:** Pakete (Tarball, deb, rpm, OpenWrt), Cross-Builds, Release-Workflow, README und Manpages
- 💤 nftables-Sets mit den Adressen erlaubter Hosts
- 💤 SSDP (UPnP) als zweites Protokoll
- 💤 FreeBSD-Backend und pfSense-Paket
- ✅ Konzept: Regelmodell, CLI, Laufzeit mit dem Maintainer besprochen

🏗️ in Arbeit · 🕵️ gebaut, in Prüfung · ☑️ bereit zur Umsetzung · 🤷 noch zu klären · 💤 später · ✅ erledigt

## Credits und Lizenzen

- Dieses Repository: MIT, Sebastian Raff.
- [avahi](https://avahi.org) (LGPL): der Reflector, den discowall ergänzt; seine Manpage ist die Quelle für das,
  was er nicht kann.
- [mdns-repeater](https://github.com/kennylevinsen/mdns-repeater) (GPL) und
  [mdns-reflector](https://github.com/vfreex/mdns-reflector) (GPL-3.0): die kleinen Reflectoren, als Vorbild für
  den Socket-Teil gelesen, kein Code übernommen.
- RFC 6762 (Multicast DNS) und RFC 6763 (DNS-Based Service Discovery): die Spezifikation.
