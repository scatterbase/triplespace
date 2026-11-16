# Hosting several wikis on one instance

One Triplespace instance can serve several wikis, called **tenants**. Each wiki has its own
host name, accounts, entity numbering and history. This guide covers deploying such an instance
in production: the host names to choose, the DNS records, TLS and the reverse proxy, the server's
settings, creating the wikis, and checking the result.

It describes what this version supports, and the last section lists what it does not support
yet. Building the binaries, creating the database and giving the server its own database
logins are covered in the [Quickstart](quickstart.md); this guide assumes you have done steps
1–4 there. The design is in [architecture 08](../architecture/08-tenants-and-instances.md).

## What this version supports

- **Every wiki is separate.** The only tenancy preset this version can create is `isolated`.
  Each wiki has its own accounts, so a person who uses two wikis has two accounts and signs in
  to each one separately. There are no farm-wide accounts, global groups or global blocks yet.
- **One host name per wiki.** The server chooses the wiki from the request's host name and
  nothing else. A wiki's base URL is a scheme and a host, such as `https://alpha.example.net`,
  with no path. Two wikis cannot share a host as `example.net/alpha` and `example.net/beta`.
- **One instance key and one database** for all the wikis. Every wiki's log is signed by the
  instance's key, and every `triplespace` command runs against the same database.

## 1. Choose the host names

You need a host name for each of these:

| Host | Example | What it serves |
|---|---|---|
| Each wiki's base | `meta.example.net`, `alpha.example.net`, `wiki.example.org` | That wiki: its pages, entities, API and sign-in. |
| The farm base | `meta.example.net` (the default) | The **primary tenant**, plus the instance-wide pages and routes, such as `Special:Tenants` and `/tenancy`. |
| The auth host | `auth.example.net` | Nothing yet except `/healthz`; see below. |

**The primary tenant** is the first wiki you create. Its `owner` group operates the whole
instance: only its owners can create more wikis. It can be an ordinary wiki with content, or a
small wiki kept only for the operators' accounts.

**The farm base** defaults to the primary tenant's base, which is the simplest choice. You can
instead give the farm a host of its own with `--farm-base`. That host then serves the primary
tenant too. Choose before you create the instance: no command changes the farm base later.

**The auth host** is required as soon as the instance has a second wiki. It must be a host of
its own, neither a wiki's nor the farm's; `tenant create` and the server both refuse an auth
base that clashes. In this version it answers only `/healthz` and returns 404 for every other
path. People sign in on each wiki's own host, as on a single-wiki instance. Reserve the name and
point DNS at it now, because later versions will move sign-in there.

Rules for every host:

- **No two may be the same.** The CLI refuses a wiki whose host is already another wiki's, the
  farm's or the auth host's.
- **The port is part of the host.** In production, use the default HTTPS port and leave it out of
  every base URL.
- **Only the exact host is served.** `www.wiki.example.org` is not `wiki.example.org`. The server
  answers any host it does not know with `421 Misdirected Request`, so redirect alternative names
  at the proxy (step 3).
- **Slugs are permanent.** Each wiki has a slug (`alpha`), and the instance has a farm slug
  (`myfarm`). Slugs are lowercase letters, digits and hyphens. They are never changed, and the
  farm slug must differ from every wiki's slug. A wiki's host cannot be changed in this version
  either, so choose names you can keep.

## 2. DNS

Point every host at the machine that runs the reverse proxy. There are two ways to do it, and
you can mix them:

- **A: a record for each host.** This works with any domain and any proxy, but every new wiki
  needs a new record.
- **B: one wildcard record for a domain.** Every subdomain of the domain resolves to the proxy,
  so a new wiki under it needs no DNS change. Pair it with a wildcard certificate (step 3) and a
  new wiki needs no proxy change either.

The examples use a server at `203.0.113.10` and `2001:db8::10`. Leave out the `AAAA` records if
the server has no IPv6 address.

### Option A: a record for each host

```text
; zone example.net
meta.example.net.     3600  IN  A     203.0.113.10
meta.example.net.     3600  IN  AAAA  2001:db8::10
alpha.example.net.    3600  IN  A     203.0.113.10
alpha.example.net.    3600  IN  AAAA  2001:db8::10
auth.example.net.     3600  IN  A     203.0.113.10
auth.example.net.     3600  IN  AAAA  2001:db8::10
```

### Option B: one wildcard record for a domain

If your wikis are subdomains of one domain, such as `meta.example.net`, `alpha.example.net` and
the auth host `auth.example.net`, two records cover all of them, now and later:

```text
; zone example.net
*.example.net.        3600  IN  A     203.0.113.10
*.example.net.        3600  IN  AAAA  2001:db8::10
```

- **One level only.** `*.example.net` covers `alpha.example.net`, but not `example.net` itself
  or `a.b.example.net`. If the bare domain is a wiki's base, give it its own `A` and `AAAA`
  records.
- **A name with records of its own is not covered.** The wildcard answers only for names that
  have no records at all. If `alpha.example.net` already has a `TXT` or `MX` record, it gets no
  address from the wildcard, so give it its own `A` and `AAAA` records too. The same rule lets
  you point other subdomains, such as `mail.example.net`, somewhere else.
- **Every subdomain reaches the server.** A name that is not a registered wiki, such as a typo,
  gets `421 Misdirected Request`, which is safe but unfriendly. To keep the wildcard away from
  your other services, give the wikis a domain or subdomain of their own, such as
  `*.wikis.example.net`.

### For both options

- **A wiki on its own domain** needs records in that domain's zone. Use a `CNAME` to one of your
  hosts:

  ```text
  ; zone example.org
  wiki.example.org.     3600  IN  CNAME meta.example.net.
  www.wiki.example.org. 3600  IN  CNAME meta.example.net.
  ```

  A domain's apex, such as `example.org` itself, cannot hold a `CNAME`, so give it `A` and `AAAA`
  records, or the `ALIAS` or `ANAME` record your DNS provider offers.
- **Certificate authorities.** If the domain publishes `CAA` records, they must allow the
  certificate authority your proxy uses, such as `0 issue "letsencrypt.org"` for Caddy's default.
  For a wildcard certificate, an `issuewild` record takes the place of `issue` if there is one.
- **Low TTLs during setup.** A TTL of 300 seconds while you set up makes mistakes cheap to fix.
  Raise it once the hosts work.

Check that each name resolves before you go on, because the proxy needs DNS in place to obtain
certificates. With a wildcard record, any name under the domain should answer:

```sh
dig +short meta.example.net alpha.example.net auth.example.net wiki.example.org
dig +short any-name-at-all.example.net
```

## 3. TLS and the reverse proxy

In production the server speaks plain HTTP on a private address and expects a proxy in front of
it that terminates TLS. The proxy must:

- **pass `Host` through unchanged**, since the host selects the wiki;
- **set `X-Forwarded-For` and `X-Forwarded-Proto`**, and discard any `X-Forwarded-*` headers that
  clients send;
- **not cache** a response that sets a cookie, or that answers a request carrying a session
  cookie or an `Authorization` header;
- **send HSTS.**

### Certificates

- **A: a certificate for each host.** The proxy obtains each certificate by answering a challenge
  over HTTP or TLS, which needs nothing beyond working DNS. Every new wiki's host has to be added
  to the proxy's configuration.
- **B: one wildcard certificate for a domain**, such as `*.example.net`, the counterpart of the
  wildcard record. Certificate authorities issue wildcards only through the DNS challenge, so the
  proxy needs an API token that can edit the domain's DNS records. Once it has the certificate, a
  new wiki under the domain needs no proxy change. Like the record, the certificate covers one
  level: `example.net` itself and `a.b.example.net` need their own.

Do not use Caddy's *on-demand TLS* to get the same effect without a wildcard certificate. It
obtains a certificate for whatever name a visitor asks for, and is only safe with an `ask`
endpoint that approves the names, which Triplespace does not provide. With a wildcard DNS record
in front of it, anyone could make your proxy request certificates for made-up subdomains until
the certificate authority's rate limits stop it.

### Caddy

[Caddy](https://caddyserver.com/) passes `Host` through, sets the forwarding headers, and obtains
and renews certificates by itself. It does not cache responses unless you add a cache module, so
the caching rule holds as written. On the same machine as the server:

**With option A**, one site block lists every host:

```caddyfile
meta.example.net, alpha.example.net, wiki.example.org, auth.example.net {
	header Strict-Transport-Security "max-age=31536000"
	reverse_proxy 127.0.0.1:8080
}

www.wiki.example.org {
	redir https://wiki.example.org{uri} permanent
}
```

Add each new wiki's host to the first line and reload Caddy.

**With option B**, one block covers every subdomain of `example.net`, and wikis on other domains
keep blocks of their own:

```caddyfile
(wiki) {
	header Strict-Transport-Security "max-age=31536000"
	reverse_proxy 127.0.0.1:8080
}

*.example.net {
	tls {
		dns cloudflare {env.CLOUDFLARE_API_TOKEN}
	}
	import wiki
}

wiki.example.org {
	import wiki
}

www.wiki.example.org {
	redir https://wiki.example.org{uri} permanent
}
```

The DNS challenge needs a Caddy built with your DNS provider's module, which the standard
download does not include. Build one with
[`xcaddy`](https://github.com/caddyserver/xcaddy), for example
`xcaddy build --with github.com/caddy-dns/cloudflare`; the
[caddy-dns](https://github.com/caddy-dns) project has modules for most providers, each with its
own `dns` line. Give Caddy a token that can edit DNS records in that one zone and nothing else,
and pass it in the environment rather than writing it into the Caddyfile.

### nginx

nginx does not obtain certificates itself; use [Certbot](https://certbot.eff.org/) or another
ACME client.

- **With option A**, use one `server` block per host, or one listing them all in `server_name`,
  with a certificate for each name.
- **With option B**, obtain the wildcard certificate with Certbot's plugin for your DNS provider,
  for example
  `certbot certonly --dns-cloudflare --dns-cloudflare-credentials /etc/letsencrypt/cloudflare.ini -d '*.example.net'`,
  and serve it from a block with `server_name *.example.net;`.

Either way, forward to the server with:

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $remote_addr;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

Setting `X-Forwarded-For` to `$remote_addr`, rather than appending to the client's header,
discards the header a client sends.

**Tell the server which proxy to trust.** The server believes `X-Forwarded-*` headers only from
a proxy it trusts. Otherwise it treats the proxy as the client, and IP blocks and rate limits
apply to the proxy's address. Trust a proxy by its address with `TRIPLESPACE_TRUSTED_PROXIES`
(step 4). If the network is shared with machines you do not trust, use a forwarder key instead:

```sh
triplespace instance forwarder create --label edge
```

It prints the key once. Have the proxy send it on every request as the
`Triplespace-Forwarder` header. In Caddy:

```caddyfile
reverse_proxy 127.0.0.1:8080 {
	header_up Triplespace-Forwarder {$TRIPLESPACE_FORWARDER_KEY}
}
```

To serve the site from a separate web tier, run `triplespace-web` beside the server and have the
proxy route between them. `triplespace-web routes --format caddy` (or `nginx` or `haproxy`)
prints the routing to import into each host's block. The *Deploying behind a proxy* section of
the [Quickstart](quickstart.md#deploying-behind-a-proxy) has the details.

## 4. Configure the server

Give the server its own environment file, as in step 6 of the [Quickstart](quickstart.md), with
its own database logins, production mode, the trusted proxy and the auth host:

```sh
cat > server.env <<'END'
TRIPLESPACE_DATABASE_URL=postgres://ts_server_login:…@localhost/triplespace
TRIPLESPACE_ACCOUNTS_DATABASE_URL=postgres://ts_accounts_login:…@localhost/triplespace
TRIPLESPACE_NOTIFY_DATABASE_URL=postgres://ts_notify_login:…@localhost/triplespace
TRIPLESPACE_KEY_FILE=triplespace.key
TRIPLESPACE_LISTEN=127.0.0.1:8080
TRIPLESPACE_MODE=production
TRIPLESPACE_TRUSTED_PROXIES=127.0.0.1,::1
TRIPLESPACE_AUTH_BASE=https://auth.example.net
END
chmod 600 server.env
```

Add the same `TRIPLESPACE_AUTH_BASE` line to `.env`, which the `triplespace` commands read as
the database owner. Then `tenant create` and `tenant setup` find it there.

Two settings must not be in production files:

- `TRIPLESPACE_DEV_TENANT`, which makes development mode serve any unknown host as one wiki;
- `TRIPLESPACE_MODE=development`, which also drops the `Secure` flag from session cookies.

**Sessions are per host.** The session cookie is set without a `Domain`, so a sign-in on
`alpha.example.net` does not carry over to `meta.example.net` or any other host.

## 5. Create the instance and the wikis

Create the instance and its primary tenant, interactively with `triplespace instance setup`, or
with flags:

```sh
triplespace instance create \
    --farm-slug myfarm \
    --tenant meta --base https://meta.example.net --name "Meta" \
    --provider wikidata \
    --owner-name Alice --owner-password-file alice.pw
```

`--tenancy` defaults to `isolated`, the only preset this version accepts. Add
`--farm-base https://farm.example.net` to give the farm a host of its own (step 1). Choose
`--provider` generously: providers cannot be added after creation.

Then add the other wikis. Each needs an auth base, which `tenant create` reads from `.env`:

```sh
triplespace tenant create --tenant alpha --base https://alpha.example.net \
    --name "Alpha" --owner-name Bob --owner-password-file bob.pw
triplespace tenant create --tenant orgwiki --base https://wiki.example.org \
    --name "Example.org wiki" --owner-name Carol --owner-password-file carol.pw
```

`triplespace tenant setup` asks the same questions one at a time. Each wiki gets its own owner
account, which holds that wiki's `owner` group.

**The owner's sign-in.** Give the owner a password with `--owner-password-file`, as above, or
let them sign in with a login they already have on this instance: `--owner-login-of` names an
account of theirs, on any wiki here, by its actor key (`{issuer code}:{user ID}`; the issuer
code is in `triplespace tenant list`), and the new owner account is bound to that account's
login. For example, to make the primary tenant's owner, user 1, the owner of a new wiki too:

```sh
triplespace tenant create --tenant beta --base https://beta.example.net \
    --name "Beta" --owner-name Alice --owner-login-of qcsbdsnqe2t3ifazvtcjsmi4si:1
```

**The operator.** A wiki is created by a request filed on behalf of an owner of the primary
tenant, recorded in the instance's log as the operator who asked for it. If the primary tenant
has more than one owner, choose one with `--operator {user ID}`; the default is the owner with
the lowest user ID. `tenant create` files the request and carries it out at once, so it ends
with the wiki created or with the reason the request was refused. A request filed from the site
is carried out by the provisioner instead; see *Creating wikis from the site* below.

To adopt an existing Wikibase as one of the wikis instead of starting it empty, see step 8 of
the [Quickstart](quickstart.md).

Start the server. It refuses to start when the instance has more than one wiki and no auth
base is set; once it runs, it serves a new wiki's host as soon as the wiki exists, with no
restart.

```sh
TRIPLESPACE_ENV_FILE=server.env triplespace-server
```

### Creating wikis from the site

An owner of the primary tenant can also request a wiki on `Special:Tenants` at the farm base.
The request is carried out by the **provisioner**, `triplespace provision`, a long-running
process beside the server. It is the only process that creates a wiki's tables and signs with
the instance key, so the public server needs neither power.

Give it a database login of its own, granted `ts_provision`, which may create a wiki's tables
and sequences through two functions the database owner owns and do nothing else beyond what the
server may:

```sql
CREATE ROLE ts_provision_login LOGIN PASSWORD '…' IN ROLE ts_provision;
```

Then run it with the instance key, the auth base, and the accounts service's login, which it
uses to bind a new owner to a login:

```sh
cat > provision.env <<'END'
TRIPLESPACE_DATABASE_URL=postgres://ts_provision_login:…@localhost/triplespace
TRIPLESPACE_ACCOUNTS_DATABASE_URL=postgres://ts_accounts_login:…@localhost/triplespace
TRIPLESPACE_KEY_FILE=triplespace.key
TRIPLESPACE_AUTH_BASE=https://auth.example.net
END
chmod 600 provision.env
TRIPLESPACE_ENV_FILE=provision.env triplespace provision
```

It waits for requests and carries out each one as soon as it is filed. `triplespace provision
--once` carries out whatever is pending and exits, for a deployment that would rather run it
from a timer.

On `Special:Tenants`, **Create a wiki** takes the slug, the base URL, the name, the owner's
account name, and a reason. Set up the wiki's DNS and proxy first (steps 2 and 3): the page does
not check them. Below the form, **Requests** shows each request's state: pending, created, or
refused with the reason, such as a slug or host already taken.

**The owner's sign-in.** Tick **The owner signs in with my login** to make yourself the new
wiki's owner with the login you already have. Otherwise, once the wiki is created, press
**Invite the owner** beside its request. The page shows an address, once, that works once for
seven days: send it to the owner, who opens it on the new wiki, chooses a password, and is
signed in as the owner.

## 6. Check the deployment

List the wikis with their hosts and issuer codes:

```sh
triplespace tenant list
```

Then check from outside, through the proxy:

```sh
# Each wiki answers on its own host: 200, or a redirect to its main page
curl -s -o /dev/null -w '%{http_code}\n' https://alpha.example.net/

# The farm base lists the instance's wikis and its tenancy policy
curl -s https://meta.example.net/w/rest.php/triplespace/v0/tenancy

# The auth host answers health checks and nothing else
curl -s https://auth.example.net/healthz

# A host the server does not know gets 421 (run this on the server itself)
curl -s -o /dev/null -w '%{http_code}\n' -H 'Host: unknown.example.net' http://127.0.0.1:8080/
```

Sign in on each wiki as its owner, on that wiki's own host. `Special:Tenants` at the farm base
lists every wiki.

## Adding a wiki later

1. **DNS:** add records for its host (step 2), unless a wildcard record already covers it.
2. **Proxy:** add the host to the proxy and reload it (step 3), unless a wildcard certificate
   already covers it.
3. Run `triplespace tenant create` (step 5), or request the wiki on `Special:Tenants` with the
   provisioner running.

With a wildcard record and a wildcard certificate for a domain, a new wiki under that domain
needs only step 3.

## Freezing, deleting and handing over a wiki

These are requests too, carried out by the provisioner, so `triplespace provision` must be
running. Each is filed from `Special:Tenants` at the farm base, by an operator who may create
wikis, with the button on the wiki's row:

- **Freeze** makes a wiki read-only. Readers see it as before; every edit is refused with
  `readonly`, as MediaWiki refuses edits to a locked wiki, and API clients see `readonly` in
  `meta=siteinfo`. **Unfreeze** undoes it.
- **Delete** freezes the wiki and erases it after a grace period, 90 days unless the instance
  says otherwise. The row shows the date. **Cancel deletion** makes the wiki active again any
  time before then. Once erased, its data is gone from the database, and its host is free for
  a new wiki; remove its DNS record and proxy entry yourself. To change the grace period:

  ```sh
  triplespace settings set --instance tenants.delete_grace_days 30
  ```

- The primary wiki cannot be frozen or deleted until another wiki has taken the primary role.

**Handing over the primary role.** An owner of the primary wiki offers the role to another
wiki under **Offer the primary role** on `Special:Tenants` at the farm base. The offer stays open
for seven days and can be withdrawn there. An owner of the wiki offered it opens
`Special:Tenants` on that wiki's own host and presses **Accept the primary role**. From then on,
that wiki's owners operate the instance, and the former primary wiki's owners own only their
own wiki. The farm base stays where it is: if it is the former primary wiki's host, the new
operators manage the instance there with accounts of that wiki, since every wiki has its own
accounts in this version.

## Not supported yet

| Missing | What it means for now |
|---|---|
| The `community` and `enterprise` presets, with farm-wide accounts, global groups and global blocks | Every wiki is isolated. A person signs up on each wiki, and a block on one wiki does not reach the others. |
| Sign-in on the auth host | People sign in on each wiki's host, and manage their login, email addresses included, on `Special:Account` there. Account emails link to that page on the wiki they were sent from. |
| A wiki at a path rather than a host | Each wiki needs its own host name. |
| Moving a single wiki to another instance | Wikis can be frozen and deleted, but not moved one at a time. The whole instance can be moved: see [Moving an instance](moving-an-instance.md). |
| Freezing, deleting or handing over a wiki from the command line | Use `Special:Tenants` or the REST API (`POST /tenancy/requests` with `act`). |
| Changing a wiki's host | Every wiki keeps the host it was created with. |
| Reading one wiki's entities from another on the same instance | Wikis do not see each other's data. The instance's provider mirrors, such as Wikidata, are held once and shared by every wiki. |
| File uploads | No media host is needed yet. |
