# 14 — Mullvad VPN and Basketball-Reference throughput

**Question.** Can routing Basketball-Reference scraping through Mullvad VPN raise
the sustainable request rate? Pure analysis — no probes were run for this note
beyond document fetches, no code or policy changes, no evasion procedures.

**Claim convention.** `[DOC]` = quoted or directly paraphrased from a primary
source listed in §5 (retrieved 2026-09-08). `[INFERENCE]` = reasoned from the
above; verify before acting on it. Claim tags follow file
[13-br-scrape-rate-limits.md](13-br-scrape-rate-limits.md), whose §§1–2 supply
the Basketball-Reference rate-limit baseline this note builds on (robots.txt
verbatim, Terms §5 items 9–11, the 20-requests/minute jail rule, and the
IP-blocking statement) — re-verified against the live pages for this note with
one fetch each, per coordination with the file-13 author.

**Verdict in one paragraph.** No. Mullvad sells privacy-flavored tunneling, not
extra rate budget: one exit IP per tunnel, at most 5 simultaneous devices per
account, all exits living in datacenter/hosting-provider address space that
bot-defense systems explicitly treat as a reputation signal. The
Basketball-Reference ceiling (20 requests/minute per session, `Crawl-delay: 3`,
jail up to a day [DOC: 13 §§1–2, re-verified]) attaches per session/IP, so N
tunnels do not multiply the polite rate — they multiply identities consuming
from the same per-IP budgets, and correlated datacenter-ASN flagging can take
all tunnels down together. The only thing a VPN "adds" after a block is a
spare IP, and using a fresh IP to continue after an explicit ban is evasion of
an access control, not a throughput strategy (§3). Real throughput gains come
from needing fewer requests at all (§4).

---

## 1. What Mullvad actually sells

### 1.1 One exit IP per tunnel

Each Mullvad connection terminates on one VPN server and presents that
server's address to the outside world. The app's own connection dashboard
shows "the VPN server's exit (out) IP address" — singular per connection
[DOC: https://mullvad.net/en/help/using-mullvad-vpn-app, "Connection
details"]. The advanced WireGuard guide likewise builds one configuration per
chosen server (a single peer Endpoint per tunnel)
[DOC: https://mullvad.net/en/help/wireguard-and-mullvad-vpn]. Multihop still
ends in a single chosen "Exit location"
[DOC: https://mullvad.net/en/help/multihop-wireguard, "How"]. So one tunnel =
one exit identity as seen by Basketball-Reference; there is no multi-IP
egress per tunnel [INFERENCE from the above].

### 1.2 What changing IP requires, and what it costs

Changing exit IP means picking another server and reconnecting. In the app
that is **Switch location** (country → city → individual server, with
unavailable servers greyed out) plus **Reconnect**, which "may reconnect you
to another Mullvad server in the selected country or city" and may change
ports [DOC: using-mullvad-vpn-app, "Switch location" / "Reconnect"]. While
reconnecting, the built-in kill switch keeps "BLOCKING INTERNET … until a
secure connection is made or reestablished," so every rotation costs a
connectivity blackout, not just a handshake
[DOC: using-mullvad-vpn-app, "Temporarily blocked Internet"]. Rough cost:
each rotation is seconds-to-tens-of-seconds of dead air per tunnel plus
operator attention (or automation the app is not designed around); account
credit itself is a flat €5/month with no bulk discount to exploit
[DOC: https://mullvad.net/en/pricing]. Nothing about reconnecting is
free-throughput [INFERENCE].

### 1.3 Concurrent-tunnel limit per account

"You can use a Mullvad VPN account on up to 5 devices" [DOC: pricing page,
"Still got questions?"]. The app enforces it at login: a sixth device must
remove an existing one before it can log in
[DOC: using-mullvad-vpn-app, "Too many devices"]. So one account yields at
most 5 simultaneous tunnels/exit IPs — the hard ceiling on any "N tunnels"
arithmetic in §2 [DOC + INFERENCE].

### 1.4 Exit IPs are datacenter ranges — and operators know it

Mullvad is explicit that its fleet is hosted infrastructure: "VPN services
such as Mullvad rent or lease servers from data centers all over the world
for their network" [DOC: multihop-wireguard, "Possible threats to single hop
VPN"]. Its server documentation confirms every server is either owned or a
rented dedicated (never virtual) machine, filterable by hosting provider and
ownership, "physically located in the locations in which they are listed"
[DOC: https://mullvad.net/en/help/server-list]. The live fleet page shows the
scale: 569 servers, 50 countries, 91 cities, 14 hosting providers
[DOC: https://mullvad.net/en/servers].

Why that matters: Cloudflare — the very platform serving
Basketball-Reference (all file-13 probes returned `server: cloudflare`
[DOC: 13 §4]) — names "data center IP addresses" and "known open proxies"
as IP-reputation signals used to detect distributed attacks
[DOC: https://blog.cloudflare.com/residential-proxy-bot-detection-using-machine-learning/,
"Residential IP proxies"]. Its per-request Bot Score (1 = certainly
automated, 99 = certainly human) fuses heuristics, machine-learning, and
JavaScript/headless-browser detection engines
[DOC: https://developers.cloudflare.com/bots/concepts/bot-score/]. A VPN
exit therefore arrives carrying the single most reputation-visible
attribute there is: a datacenter ASN shared with thousands of strangers'
bots [INFERENCE from the two DOCs above].

---

## 2. Throughput math: why N tunnels ≠ N × the polite rate

Take the documented ceiling as fixed: 20 requests/minute per session on
Basketball-Reference ("regardless of bot type and construction and pages
accessed," violators "in jail for up to a day" [DOC: 13 §2.4, re-verified
at https://www.sports-reference.com/bot-traffic.html]), plus `Crawl-delay: 3`
[DOC: 13 §1, re-verified at
https://www.basketball-reference.com/robots.txt]. The repo's ≥3.5 s spacing
already sits ~12% under both signals [DOC: 13, verdict].

1. **Per-IP budgets still apply behind the VPN.** Sports Reference's
   escalation language is IP-keyed: "If we notice excessive activity from a
   particular IP address we will be forced to take appropriate measures,
   which will include, but not be limited to, blocking that IP address"
   [DOC: https://www.sports-reference.com/data_use.html, re-verified].
   Each Mullvad tunnel owns exactly one exit IP (§1.1), so each tunnel
   inherits its own 20/min budget — five tunnels do not create a 100/min
   entitlement, they create five separately jailable 20/min sessions, all
   attributable to one operator's scrape [INFERENCE].
2. **Reconnect latency serializes rotation.** IP changes are not free:
   every switch pays a reconnect blackout (§1.2), and a single-host
   sequential scraper cannot pipeline through a tunnel that is down.
   Sustained throughput is bounded by uptime × per-IP budget, and
   rotation *subtracts* uptime [INFERENCE].
3. **The correlated-failure mode.** Cloudflare's models score every
   request and explicitly improved at catching "attacks that originate
   from cloud providers" (v8 detects "20% more bots from cloud providers")
   [DOC: Cloudflare ML blog]. Five exits from the same handful of hosting
   providers share ASN/provider reputation; per-request ML scoring plus
   ASN-level signals mean one flagged tunnel predicts the
   neighboring tunnels' scores. The failure mode is not "one IP jailed,
   four keep going" — it is all tunnels degrading together, followed by
   the documented IP-block escalation [INFERENCE from the DOCs above].
   That operators keep standing "bad IP" lists and throttle-or-block
   "suspicious IP addresses" as routine practice is a matter of court
   record (LinkedIn's Sentinel/Org Block systems, described at
   hiQ pp. 10–11 [DOC: §5]).
4. **Rotation is named as attacker behavior.** Cloudflare's own engineers
   write that "IP address rotation allows attackers to directly bypass
   traditional defenses such as IP reputation and IP rate limiting" —
   listing rotation alongside the abuse, not the legitimate use
   [DOC: Cloudflare ML blog]. Expect defenses to be tuned accordingly
   [INFERENCE].

Net: a VPN multiplies *identities*, while the binding constraints (per-IP
rate budget, per-request bot scoring, ASN reputation) are all keyed to
exactly the thing being multiplied. Sustainable throughput is unchanged;
ban surface area grows [INFERENCE].

---

## 3. ToS / circumvention analysis

> General information only — not legal advice. Statute and case holdings
> below are quoted briefly from the primary texts in §5.

### 3.1 What Sports Reference's terms actually say (and don't say)

Full-text read of the Terms of Use on 2026-09-08 confirms the sibling
finding: **the ToS contains no "circumvention" clause by that name.**
The closest language is §5 item 13: "attempt to probe, scan, or test the
vulnerability of the Site or breach any implemented security or
authentication measures, regardless of your motives or intent"
[DOC: https://www.sports-reference.com/termsofuse.html §5, re-verified;
cf. 13 §2]. The directly relevant prohibitions, quoted in full in file 13
and re-verified here, are:

- Automated access "without our express written permission, use any
  automated means to access or use the Site, including scripts, bots,
  scrapers, data miners, or similar software, in a manner that adversely
  impacts site performance or access" (ToS §5 item 9 [DOC: termsofuse.html
  §5; 13 §2.1]).
- Disruption: "attempt to or actually disrupt, impair, or interfere with
  the Site" and "attempt to interfere with or disrupt access to or use of
  the Site by any user" (§5 items 12, 14 [DOC: termsofuse.html §5]).
- Enforcement: session jail "for up to a day" for exceeding the rate rule
  [DOC: bot-traffic.html; 13 §2.4] and IP blocking for "excessive
  activity from a particular IP address" [DOC: data_use.html; 13 §2.4].

Deliberately switching exit IPs to keep scraping after a session jail or
IP block lands squarely inside the conduct these clauses aim at —
continuing automated access the operator just told you to stop — even
though no single sentence uses the word "circumvention" [INFERENCE from
the clause texts].

### 3.2 CFAA exposure after an explicit ban (general information)

The federal computer-crime statute punishes anyone who "intentionally
accesses a computer without authorization or exceeds authorized access,
and thereby obtains … information from any protected computer," 18 U.S.C.
§ 1030(a)(2)(C) [DOC: https://www.law.cornell.edu/uscode/text/18/1030].
Two holdings shape how courts read those two phrases for public-website
scraping:

- *Van Buren v. United States*, 593 U.S. 374 (2021), holds that a person
  "exceeds authorized access" only when obtaining "information located
  in particular areas of the computer — such as files, folders, or
  databases — that are off-limits," adopting a "gates-up-or-down inquiry
  — one either can or cannot access a computer system, and one either
  can or cannot access certain areas within the system"
  [DOC: syllabus, https://www.law.cornell.edu/supremecourt/text/19-783].
  Misusing access one already has, for a forbidden purpose, is not
  "exceeding authorized access" under this holding [DOC: same].
- *hiQ Labs, Inc. v. LinkedIn Corp.*, 31 F.4th 1180 (9th Cir. 2022), held
  on preliminary-injunction review that a scraper of *public* profiles
  (no login gate) "raised a serious question" that the CFAA's "without
  authorization" concept is "inapplicable where … prior authorization is
  not generally required but a particular person — or bot — is refused
  access," because "applying the 'gates' analogy to a computer hosting
  publicly available webpages, that computer has erected no gates to
  lift or lower in the first place," with *Van Buren* "reinforc[ing]"
  that reading [DOC: slip op. at 29, 34–36,
  https://cdn.ca9.uscourts.gov/datastore/opinions/2022/04/18/17-16783.pdf].
  Limits of that holding, stated honestly: it is Ninth Circuit,
  preliminary-injunction posture ("serious questions," not final
  merits), and the same opinion distinguishes *Facebook v. Power
  Ventures*, where circumventing IP barriers to reach
  password-authenticated content *did* support CFAA liability
  [DOC: slip op. at 36–37]. The opinion also preserves non-CFAA theories
  — "state law trespass to chattels claims may still be available" — and
  expressly notes the injunction "does not preclude [the operator] from
  … employ[ing] anti-bot measures to prevent, e.g., harmful intrusions
  or attacks on its server" [DOC: slip op. at 41–43].

Applied generally (not as advice): polite scraping of ungated pages sits
in hiQ's most defendant-favorable category, while rotating IPs to defeat
an explicit block moves *toward* the barrier-circumvention fact pattern
courts treat less favorably — and state-law and contract theories remain
available to operators regardless [INFERENCE from the holdings above;
not legal advice].

---

## 4. Verdict and what legitimately raises throughput instead

**Can Mullvad raise the sustainable Basketball-Reference request rate?
No.** It cannot multiply a per-IP/per-session budget (§2); its exits
carry datacenter-ASN reputation that bot defenses score against (§1.4);
its account cap bounds the scheme at 5 concurrent identities (§1.3); and
the one mechanism it does offer post-block — a fresh IP — is evasion of
an explicit access control, prohibited in substance by the ToS enforcement
clauses and the costliest possible fact pattern under §3
[INFERENCE from §§1–3].

What actually raises throughput — every item compliant with the file-13
posture — reduces requests or spreads them, never identities:

- **Narrower page set.** Fetch only robot-permitted schedule/boxscore
  paths; never follow links into robots.txt-disallowed sections (file 13
  lists them; `/leagues/` and `/boxscores/` are permitted)
  [DOC: 13 §1]. Fewer pages is the only throughput gain with zero ban
  risk [INFERENCE].
- **Caching and resumability.** Skip files already on disk, resume rather
  than restart killed runs, keep the identifying UA and 3.5–6 s spacing
  [DOC: 13 §5]. Re-fetches are pure waste against a rate budget
  [INFERENCE].
- **Off-peak scheduling.** Run the multi-day one-shot build when human
  load is lowest; the operator's stated motive is protecting human
  browsing experience and ad revenue [DOC: bot-traffic.html; 13 §2.4].
  File 13's arithmetic bounds the prize honestly: ceiling-vs-polite
  saves under half a day on a ~3-day build [DOC: 13 §3].
- **Ask for data instead of scraping it.** Custom extracts "start at a
  minimum of $5,000" via the Feedback Form
  [DOC: data_use.html ("If you are willing to meet our minimum fee,
  please contact us via our Feedback Form")]. That is the operator's
  published legitimate path to bulk data [DOC: same].

---

## 5. Sources (primary only)

- https://mullvad.net/en/pricing — €5/month flat; "up to 5 devices."
- https://mullvad.net/en/help/using-mullvad-vpn-app — device cap
  enforcement; single exit IP in Connection details; Switch
  location / Reconnect / kill-switch blackout.
- https://mullvad.net/en/help/wireguard-and-mullvad-vpn — one-server
  WireGuard configuration model.
- https://mullvad.net/en/help/multihop-wireguard — single Exit location;
  "rent or lease servers from data centers."
- https://mullvad.net/en/help/server-list — owned vs rented dedicated
  servers; provider/ownership filters; physical-location statement.
- https://mullvad.net/en/servers — fleet scale (569 servers, 50
  countries, 91 cities, 14 providers).
- https://developers.cloudflare.com/bots/concepts/bot-score/ — 1–99
  per-request score; heuristics / ML / JavaScript-detection engines.
- https://blog.cloudflare.com/residential-proxy-bot-detection-using-machine-learning/
  — datacenter IPs as reputation signals; rotation as bypass behavior;
  cloud-provider-origin detection gains.
- https://www.basketball-reference.com/robots.txt — `Crawl-delay: 3`
  (re-verified; verbatim in 13 §1).
- https://www.sports-reference.com/termsofuse.html — §5 items 9, 12–14;
  §6 robots.txt expectation (re-verified; "Last Updated: May 19, 2023").
- https://www.sports-reference.com/data_use.html — IP-blocking statement;
  $5,000 custom-extract minimum; Feedback Form contact (re-verified).
- https://www.sports-reference.com/bot-traffic.html — 20/min rule, jail
  up to a day (re-verified; "Update: May 29, 2024").
- https://www.law.cornell.edu/uscode/text/18/1030 — 18 U.S.C.
  § 1030(a)(2)(C), (e)(2)(B), (g).
- https://www.law.cornell.edu/supremecourt/text/19-783 — *Van Buren*
  syllabus (gates-up-or-down holding, decided June 3, 2021).
- https://cdn.ca9.uscourts.gov/datastore/opinions/2022/04/18/17-16783.pdf
  — *hiQ v. LinkedIn* slip opinion (filed Apr. 18, 2022; "without
  authorization" serious-question holding; Power Ventures distinction;
  trespass/anti-bot-measures preservation).
- [13-br-scrape-rate-limits.md](13-br-scrape-rate-limits.md) §§1–2, 4–5 —
  sibling baseline this note cites rather than re-probing.
