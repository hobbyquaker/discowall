# discowall

[:uk: English README](README.md)

[![CI](https://github.com/hobbyquaker/discowall/actions/workflows/ci.yml/badge.svg)](https://github.com/hobbyquaker/discowall/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!CAUTION]
> discowall ist in Entwicklung. Der Reflector mit Regelkette und CLI steht (P0, P1), ist aber noch auf keinem echten Router gelaufen.

discowall ist eine **Discovery-Firewall** für einen Linux-Router: Es leitet mDNS-Pakete (Bonjour, Zeroconf,
RFC 6762) von einer Netzwerkschnittstelle auf andere weiter, so wie es der Reflector von avahi-daemon tut, aber
nach **Regeln**: welche Hosts und welche Dienste aus welchem Netz in welchem anderen Netz sichtbar werden, als
Whitelist oder Blacklist, nach Quelle, Ziel und Richtung. Eine einfache Form von iptables für `.local`. Dazu eine
CLI, die zeigt, welche Ankündigungen auf welcher Schnittstelle ankommen und welche Regel sie passieren lassen, und
die sich in Skripten benutzen lässt. SSDP (UPnP) wird nach denselben Regeln weitergeleitet.

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

## Installation

Die Releases auf GitHub enthalten statische Binaries für x86_64, aarch64 und armv7 (musl, keine Abhängigkeiten)
als Tarball mit Manpages, systemd-Unit und Beispielen sowie als `.deb`- und `.rpm`-Paket. Ein OpenWrt-Paket
(`contrib/openwrt/`) ist geschrieben, aber noch auf keinem echten Build-Host gebaut.

```sh
# Debian, Ubuntu, Armbian
sudo dpkg -i discowall_<version>_<arch>.deb
# Fedora
sudo rpm -i discowall-<version>-1.<arch>.rpm
# jedes Linux: der Tarball
tar xzf discowall-<version>-<target>.tar.gz && cd discowall-*
sudo install -m 755 discowalld discoctl /usr/local/bin/
sudo install -d /etc/discowall && sudo install -m 644 discowall.toml rules.toml /etc/discowall/
sudo install -m 644 discowalld.service /etc/systemd/system/ && sudo systemctl daemon-reload
# aus den Quellen (Rust stable)
cargo install --git https://github.com/hobbyquaker/discowall discowalld discoctl
```

Dann die Schnittstellen in `/etc/discowall/discowall.toml` eintragen, Regeln in `/etc/discowall/rules.toml`
schreiben (oder mit `discoctl` anlegen und mit `discoctl save` sichern) und die Unit starten:

```sh
sudo systemctl enable --now discowalld
discoctl status
```

Der Daemon läuft als dynamischer Benutzer ohne Capabilities: Port 5353 und Multicast-Beitritte brauchen keine.
avahi-daemon darf auf demselben Host weiterlaufen; discowall leitet nur weiter und antwortet nie für den Router.

## Konfiguration

`/etc/discowall/discowall.toml`:

```toml
interfaces = ["lan", "iot"]          # Linux-Namen; eine noch nicht vorhandene Schnittstelle wird abgewartet
#ipv4 = true
#ipv6 = true
#ssdp = true                         # auch SSDP (UPnP, 239.255.255.250:1900) nach denselben Regeln
#control_socket = "/run/discowall/control.sock"
#rules_file = "/etc/discowall/rules.toml"
#dedup_window_ms = 500               # ein identisches Paket in diesem Fenster wird verworfen (zweiter Reflector)
#strip_link_local = true             # fe80::/10- und 169.254/16-Adressrecords entfernen: im anderen Netz tot
#legacy_window_ms = 500              # Einmal-Anfragen (dig) werden stellvertretend gestellt; 0 verwirft sie
```

`discowalld --check` prüft die Datei. `SIGHUP` lädt die Regeldatei neu, nicht die Schnittstellen.

## Regeln

Eine geordnete Kette, die erste Übereinstimmung gewinnt, eine Default-Policy. Eine Regel hat Kriterien und eine
Aktion; ein weggelassenes Kriterium passt auf alles, eine Liste bedeutet "eines davon". Namen werden ohne ihren
`.local`- bzw. `._service._proto.local`-Schwanz verglichen, ohne Beachtung der Groß- und Kleinschreibung, mit
`*`- und `?`-Globs.

| Kriterium | Bedeutung | Beispiel |
| --- | --- | --- |
| `proto` | `mdns`, `ssdp`; fehlt: beide | `proto ssdp` |
| `in` | Schnittstelle, auf der das Paket ankam | `in iot` |
| `out` | Schnittstelle, auf die es gesendet würde | `out lan` |
| `src` | ursprüngliche Quelladresse oder Präfix | `src 192.168.30.0/24,fd00::/8` |
| `kind` | `query`, `probe`, `response`, `announce` | `kind response,announce` |
| `service` | Diensttypen | `service _hap._tcp,_airplay._tcp` |
| `instance` | Instanznamen | `instance "living room*"` |
| `host` | Hostnamen | `host bridge,printer?` |
| `rrtype` | Record-Typen, bei Anfragen der der Frage | `rrtype AAAA` |
| Aktion | `accept` oder `drop`, `log` schreibt pro Treffer eine Journal-Zeile | `accept log` |

Bei SSDP ist `service` das NT/ST (`upnp:rootdevice`, `urn:schemas-upnp-org:device:MediaRenderer:1`, `ssdp:all`),
`instance` die `uuid:…` des Geräts und `host` der Host seiner LOCATION-URL; `kind` ist `query` für M-SEARCH,
`announce` für alive/byebye/update und `response` für eine Suchantwort.

Eine Regel auf einen Host oder Dienst gilt für das ganze Bündel, das ein Dienst ist: seine PTR-, SRV-, TXT- und
A/AAAA-Records, die in getrennten Paketen kommen. Der Daemon führt eine Tabelle dessen, was jede Schnittstelle
angekündigt hat (Instanz → Dienst und Host, Host → Adressen), und ordnet jeden Record darüber zu. Ein Paket wird
nur mit den durchgelassenen Records neu gebaut; eine Anfrage ohne verbliebene Frage oder eine Antwort ohne
verbliebene Antwort wird nicht gesendet.

Die Dateiform, `/etc/discowall/rules.toml`:

```toml
default = "drop"

[[rule]]
in = "lan"
out = "iot"
kind = ["query"]
service = ["_hap._tcp"]
action = "accept"

[[rule]]
in = "iot"
out = "lan"
kind = ["response", "announce"]
host = ["bridge"]
action = "accept"
log = true
```

Dasselbe über die CLI (die Kette im Daemon ändert sich sofort; `save` schreibt die Datei, `apply` lädt sie):

```sh
discoctl rule add in lan out iot kind query service _hap._tcp accept
discoctl rule add in iot out lan kind response,announce host bridge accept log
discoctl rule insert 1 src 192.168.40.0/24 drop comment "Gäste sehen nichts"
discoctl rule list
discoctl rule test in iot out lan kind announce host bridge      # → accept (rule 2)
discoctl rule del 1
discoctl save
```

Whitelist: `default = "drop"` plus accept-Regeln. Blacklist: `default = "accept"` plus drop-Regeln. Eine fehlende
Regeldatei bedeutet `drop` ohne Regeln: nichts geht durch, bis du es sagst.

### Rezepte

```sh
# HomeKit: das LAN darf im IoT-Netz suchen, nur die Bridge antwortet
discoctl rule add in lan out iot kind query service _hap._tcp accept
discoctl rule add in iot out lan kind response,announce host bridge accept

# AirPlay- und Spotify-Connect-Lautsprecher in "media" sind vom LAN aus sichtbar, nichts vom LAN in "media"
discoctl rule add in lan out media kind query service _airplay._tcp,_raop._tcp,_spotify-connect._tcp accept
discoctl rule add in media out lan kind response,announce service _airplay._tcp,_raop._tcp,_spotify-connect._tcp accept

# Drucker überall
discoctl rule add kind query service _ipp._tcp,_ipps._tcp,_printer._tcp,_pdl-datastream._tcp accept
discoctl rule add kind response,announce service _ipp._tcp,_ipps._tcp,_printer._tcp,_pdl-datastream._tcp accept

# Chromecast / Google Cast vom LAN ins IoT-Netz, aber nicht aus dem Gästenetz
discoctl rule add in guest drop
discoctl rule add in lan out iot kind query service _googlecast._tcp accept
discoctl rule add in iot out lan kind response,announce service _googlecast._tcp accept

# UPnP: das LAN darf im IoT-Netz suchen, der Media-Renderer meldet sich zurück
discoctl rule add proto ssdp in lan out iot kind query accept
discoctl rule add proto ssdp in iot out lan kind announce,response service urn:schemas-upnp-org:device:MediaRenderer:1,upnp:rootdevice accept

# Sonos: ein Host, nach Namen
discoctl rule add in lan out media kind query service _sonos._tcp accept
discoctl rule add in media out lan kind response,announce host "sonos*" accept
```

Discovery ist nicht der Datenpfad. Hat ein Client einen Dienst gefunden, verbindet er sich mit der Adresse aus dem
A-Record, und diese Verbindung braucht ihre eigene Firewall-Regel (TCP 51826 für HomeKit, 631 für IPP, 7000 und
7100 für AirPlay, 8008 und 8009 für Cast, 1400 für Sonos usw.). Eine spätere Version soll die Adressen erlaubter
Hosts in nftables-Sets pflegen, damit eine statische Regel pro Netzpaar reicht.

## discoctl

```
discoctl status                 Schnittstellen, Zustand, Gruppen, Adressen, Zähler
discoctl rule list|add|insert N|del N|flush|test …
discoctl save | apply           Regeldatei schreiben | neu laden
discoctl hosts                  die Tabelle: Hosts, Adressen, Instanzen, je Schnittstelle
discoctl services               die Instanzen: Diensttyp, Host, Port, Rest-TTL
discoctl stats                  Zähler je Schnittstelle und je Regel
discoctl watch [--iface X]      live: eine Zeile pro Paket mit seinen Entscheidungen
```

Jeder Befehl kennt `--json` für Skripte; `--socket` oder `DISCOWALL_SOCKET` nennt den Steuer-Socket. Exit-Codes:
0 ok, 1 der Daemon hat abgelehnt, 2 Bedienfehler, 3 Daemon nicht erreichbar. Eine `watch`-Zeile:

```
14:02:11.482 iot      10.1.0.5:5353         announce  _hap._tcp "living room" bridge.local   -> lan sent(2)  guest dropped
```

Das Ergebnis je Zielschnittstelle ist `sent`, `partial` (einige Records verworfen, die Zahl folgt), `dropped`,
`down` oder bei Einmal-Anfragen `asked` und `relayed`. Die entscheidenden Regeln stehen in Klammern.

## Wie es funktioniert

- Ein UDP-Socket je Adressfamilie auf Port 5353, an die Wildcard-Adresse gebunden mit `SO_REUSEADDR` und
  `SO_REUSEPORT`, damit avahi den Port auch halten kann; `IP_PKTINFO` sagt, auf welcher Schnittstelle ein Paket
  ankam, und dieselbe Control-Message wählt beim Senden die Ausgangsschnittstelle. Die Gruppe wird auf jeder
  konfigurierten Schnittstelle beigetreten, sobald rtnetlink sie als up meldet. Keine Raw-Sockets, keine
  Capabilities.
- Jedes Paket wird geparst (hickory-proto). Die Host-Tabelle lernt aus jedem Paket, weitergeleitet oder nicht.
  Für jede Zielschnittstelle entscheidet die Kette pro Frage und pro Record; das Paket wird neu kodiert, wenn
  etwas verworfen wurde, das QU-Bit gelöscht, Link-local-Adressrecords entfernt, die TTL ist 255.
- Schleifen: nie zurück auf die Eingangsschnittstelle, Pakete von den eigenen Adressen des Routers werden
  verworfen, ein identisches Paket im Dedup-Fenster ebenso.
- SSDP (UPnP, 239.255.255.250:1900 / ff02::c) nutzt ein zweites Socket-Paar. NOTIFY-Nachrichten werden nach den
  Regeln weitergeleitet; ein M-SEARCH läuft immer über den Proxy, weil UPnP-Antworten per Unicast an den Port des
  Suchenden gehen: Der Daemon sucht auf den erlaubten Schnittstellen von einem Ephemeral-Socket, sammelt die
  Antworten MX Sekunden lang und gibt sie von Port 1900 an den Suchenden zurück. Eine LOCATION auf einer
  Link-local-Adresse wird verworfen.
- Eine Anfrage von einem Ephemeral-Port (`dig @224.0.0.251 -p 5353`, Einmal-Bibliotheken) lässt sich nicht
  spiegeln: Der Daemon fragt auf den erlaubten Schnittstellen von einem eigenen Ephemeral-Socket, sammelt die
  Unicast-Antworten ein kurzes Fenster lang, schickt sie durch die Regeln und gibt sie per Unicast von Port 5353
  an den Fragenden zurück.
- Die Tests laufen in Linux-Netzwerk-Namespaces: ein Router-Namespace mit dem Daemon zwischen zwei veth-Blättern,
  echtes Multicast auf beiden Adressfamilien, Regeln über `discoctl`, der Legacy-Proxy Ende-zu-Ende.

## Stand

Die Recherche vom 10. Oktober 2026 (Überblick über avahi und die vorhandenen Reflectoren, Entwurf, Bewertung von
Rust, Aufwandsschätzung, Protokoll der Entscheidungen) steht in
[mdns-router-research.md](mdns-router-research.md). Die Entscheidungen sind in
[roadmap-items/discowall/decisions.md](roadmap-items/discowall/decisions.md), die Aufgaben einzeln in
[roadmap-readmes/discowall.md](roadmap-readmes/discowall.md), die offenen Fragen in
[roadmap-items/discowall/overview.md](roadmap-items/discowall/overview.md).

## Roadmap

- 🕵️ **P0:** Cargo-Projekt, Sockets, Schnittstellen-Überwachung per Netlink, einfacher Reflector mit
  Schleifenschutz, systemd-Unit
- ✅ **Lab:** Koexistenz mit avahi-daemon auf Port 5353 messen (gemessen 2026-10-13: Multicast an beide, Unicast an den zuletzt gebundenen Socket, kein Sturm)
- 🕵️ **P1:** Parser, Host-Tabelle und Record-Bündel, Regelkette, Steuer-Socket, `discoctl rule …`
- 🕵️ **P2:** `discoctl watch`, `hosts`, `services`, `stats`, `status`; Zähler pro Regel, `log`
- 🕵️ **P3:** Stellvertreter für einmalige Anfragen und das QU-Bit, Link-local-AAAA entfernen, IPv6 durchgehend,
  Deduplizierung, Fuzzing, Härtung der Unit
- 🕵️ **P4:** Pakete (Tarball, deb, rpm, OpenWrt), Cross-Builds, Release-Workflow, README und Manpages
- 💤 nftables-Sets mit den Adressen erlaubter Hosts
- 🕵️ SSDP (UPnP) als zweites Protokoll
- 💤 FreeBSD-Backend und pfSense-Paket
- 🏗️ Proxmox-Lab für End-to-End-Tests (drei LXC an einer VLAN-fähigen Bridge, lab.py)
- 🕵️ avahi im Namespace-Test auf GitHub
- ✅ B-1: Log-Ausgabe trug ANSI-Farbcodes im Journal (im Lab gefunden und behoben)
- ✅ B-2: Dedup verwarf den IPv4-Zwilling einer Anfrage, die avahi auf beiden Familien zugleich sendet (im Lab gefunden und behoben)
- ✅ B-3: `discoctl save` scheiterte unter der mitgelieferten Unit; discoctl schreibt die Regeldatei jetzt selbst (im Lab gefunden und behoben)
- 🤷 LuCI-App für das OpenWrt-Paket (Web-UI für Regeln, Hosts, Dienste, Status)
- 🤷 pfSense-Paket und Web-UI (FreeBSD-Port, packagegui, Regelseite; OPNsense-Plugin offen)
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
