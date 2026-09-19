#!/usr/bin/env python3
"""Differential test: Java komga vs komga-rs over the same fixture library.

Usage:
  python3 diff.py --java-jar <bootJar> --rust-bin <komga-server> [--workdir /tmp/komga-diff] [--skip-start]

Starts both servers on the same fixture library (each with its own config dir),
walks the endpoint list, and diffs status codes, JSON bodies (with volatile
fields normalized), and key response headers.
"""

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error
import zipfile

KOMGA_RESOURCES = os.path.expanduser("~/src/komga/komga/src/test/resources")
FIXTURES = [
    ("archives/zip.zip", "manga/berserk/v01.cbz"),
    ("archives/rar4.rar", "manga/berserk/v02.cbr"),
    ("archives/epub3.epub", "manga/solo/book.epub"),
]

# ---------------------------------------------------------------------------
# normalization
# ---------------------------------------------------------------------------

# Crockford Base32 (no I/L/O/U), 13 chars: komga TSID
TSID_RE = re.compile(r"\b[0-9A-HJKMNP-TV-Z]{13}\b")
HOST_RE = re.compile(r"http://localhost:\d+")

TIME_KEYS = {
    "timestamp", "updated", "lastModified", "created", "modified", "readDate",
    "createdDate", "lastModifiedDate", "fileLastModified", "read_date",
    "startTime", "published", "publishedAt", "date", "lastModifiedTime",
    "mostRecentReadDate", "file_last_modified", "lastReadDate", "releaseDate",
}

# ISO-8601 date-time with optional fraction and offset (Z or ±HH:MM)
ISO_TIME_RE = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})")


def is_tsid(value):
    return isinstance(value, str) and TSID_RE.fullmatch(value) is not None


class Normalizer:
    """Maps volatile values (TSIDs, hosts, timestamps) to stable placeholders.

    `aliases` maps real entity ids to semantic names (e.g. series id -> "SERIES:berserk"),
    so the same entity gets the same placeholder on both sides regardless of scan order.
    """

    def __init__(self, aliases=None):
        self.aliases = aliases or {}
        self.tsid_map = {}
        self.tsid_counter = 0

    def tsid(self, value):
        if value in self.aliases:
            return f"<{self.aliases[value]}>"
        if value not in self.tsid_map:
            self.tsid_counter += 1
            self.tsid_map[value] = f"<TSID-{self.tsid_counter}>"
        return self.tsid_map[value]

    def normalize(self, obj, key=""):
        if isinstance(obj, dict):
            out = {}
            for k, v in obj.items():
                if k == "serverPort":
                    out[k] = "<PORT>"
                else:
                    out[k] = self.normalize(v, k)
            return out
        if isinstance(obj, list):
            return [self.normalize(v, key) for v in obj]
        if isinstance(obj, str):
            if key in TIME_KEYS and re.fullmatch(r"\d{4}-\d{2}-\d{2}.*", obj):
                return "<TIME>"
            # TSIDs may appear as whole values or embedded in URLs/text
            obj = TSID_RE.sub(lambda m: self.tsid(m.group(0)), obj)
            obj = ISO_TIME_RE.sub("<TIME>", obj)
            if HOST_RE.search(obj):
                obj = HOST_RE.sub("http://HOST", obj)
            return obj
        return obj


def normalize_body(text, aliases=None):
    # response bodies arrive as bytes; JSON normalization happens on the parsed value,
    # text normalization (XML and other non-JSON) on the decoded string
    if isinstance(text, bytes):
        text = text.decode("utf-8", errors="replace")
    try:
        return Normalizer(aliases).normalize(json.loads(text))
    except (json.JSONDecodeError, ValueError):
        return Normalizer(aliases).normalize(text)


# ---------------------------------------------------------------------------
# endpoint-specific comparators
# ---------------------------------------------------------------------------

def shape(obj):
    """A structural skeleton of a JSON value: dict keys and list lengths only."""
    if isinstance(obj, dict):
        return {k: shape(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [shape(v) for v in obj]
    return type(obj).__name__


def compare_actuator_health(java_body, rust_body):
    problems = []
    for side, body in (("java", java_body), ("rust", rust_body)):
        if body.get("status") != "UP":
            problems.append(f"{side} status != UP")
    for ds in ("sqliteDataSourceRO", "sqliteDataSourceRW", "tasksDataSourceRO", "tasksDataSourceRW"):
        for side, body in (("java", java_body), ("rust", rust_body)):
            component = body.get("components", {}).get("db", {}).get("components", {}).get(ds)
            if not component:
                problems.append(f"{side} missing data source {ds}")
            elif component.get("details", {}).get("database") != "SQLite" or component.get("details", {}).get("validationQuery") != "isValid()":
                problems.append(f"{side} {ds} details differ: {component.get('details')}")
    for side, body in (("java", java_body), ("rust", rust_body)):
        disk = body.get("components", {}).get("diskSpace", {}).get("details", {})
        for key in ("total", "free", "threshold", "exists"):
            if key not in disk:
                problems.append(f"{side} diskSpace missing {key}")
    return problems


def compare_actuator_info(java_body, rust_body):
    problems = []
    for section in ("git", "build", "java", "os"):
        for side, body in (("java", java_body), ("rust", rust_body)):
            if section not in body:
                problems.append(f"{side} missing info section {section}")
    if "git" in java_body and "git" in rust_body:
        for key in ("branch", "commit"):
            for side, body in (("java", java_body), ("rust", rust_body)):
                if key not in body["git"]:
                    problems.append(f"{side} git missing {key}")
        for side, body in (("java", java_body), ("rust", rust_body)):
            if "id" not in body["git"].get("commit", {}) or "time" not in body["git"].get("commit", {}):
                problems.append(f"{side} git.commit missing id/time")
    for key in ("artifact", "name", "version", "group"):
        for side, body in (("java", java_body), ("rust", rust_body)):
            if key not in body.get("build", {}):
                problems.append(f"{side} build missing {key}")
    for section in ("java", "os"):
        sj, sr = shape(java_body.get(section, {})), shape(rust_body.get(section, {}))
        if sj != sr:
            problems.append(f"{section} shape differs ({sj} vs {sr})")
    return problems


def compare_actuator_metrics(java_body, rust_body):
    problems = []
    j_names = set(java_body.get("names", []))
    r_names = set(rust_body.get("names", []))
    # MultiGauge-backed komga.* names (e.g. komga.sidecars) only appear on the Java side
    # when their row has data; tolerate them being absent there
    tolerated_missing = {"komga.sidecars"}
    if not r_names <= (j_names | tolerated_missing):
        problems.append(
            f"rust names not a subset of java names: {sorted(r_names - j_names - tolerated_missing)}"
        )
    for required in ("process.uptime", "process.start.time"):
        if required not in r_names:
            problems.append(f"rust missing {required}")
    return problems


def compare_actuator_scheduledtasks(java_body, rust_body):
    problems = []
    for key in ("cron", "fixedDelay", "fixedRate", "custom"):
        if key not in rust_body:
            problems.append(f"rust missing {key}")
    r_targets = [t.get("runnable", {}).get("target", "") for t in rust_body.get("fixedRate", [])]
    for required in ("SseController.heartbeat", "SseController.taskCount", "AuthenticationActivityCleanupController.cleanup"):
        if required not in r_targets:
            problems.append(f"rust missing fixedRate target {required}")
    for task in rust_body.get("fixedRate", []):
        if task.get("initialDelay") != task.get("interval"):
            problems.append(f"rust task initialDelay != interval: {task}")
    return problems


ACTUATOR_RULES = {
    "/actuator/health": compare_actuator_health,
    "/actuator/info": compare_actuator_info,
    "/actuator/metrics": compare_actuator_metrics,
    "/actuator/scheduledtasks": compare_actuator_scheduledtasks,
}


def sort_feed_lists(obj, key=""):
    """Recursively sorts OPDS feed lists (navigation/publications/groups/facets) by title or href.

    Latest-Books/Latest-Series lists are ordered by createdDate/lastModified, which depend on
    each implementation's scan traversal order; the set of items must match, the order need not.
    links/images/readingOrder keep their order — only known feed collection keys are sorted.
    """
    if isinstance(obj, dict):
        return {k: sort_feed_lists(v, k) for k, v in obj.items()}
    if isinstance(obj, list):
        items = [sort_feed_lists(v) for v in obj]
        if key not in ("navigation", "publications", "groups", "facets", "entries"):
            return items

        def sort_key(item):
            if isinstance(item, dict):
                for k in ("title", "href", "name", "id"):
                    if k in item:
                        return str(item[k])
                if "metadata" in item and isinstance(item["metadata"], dict) and "title" in item["metadata"]:
                    return str(item["metadata"]["title"])
            return json.dumps(item, sort_keys=True)

        return sorted(items, key=sort_key)
    return obj


# paths whose feed lists are order-insensitive (time-ordered lists that depend on scan order)
UNORDERED_FEED_PREFIXES = (
    "/opds/v1.2/books/latest",
    "/opds/v1.2/series/latest",
    "/opds/v2/libraries/books/latest",
    "/opds/v2/libraries/series/latest",
    "/opds/v2/catalog",
    "/opds/v2/libraries",
)


def sort_xml_entries(text):
    """Sorts the <entry> blocks of an Atom feed (OPDS v1.2) so time-ordered feeds can be
    compared as sets. Entries do not nest, so a plain split is safe."""
    if not isinstance(text, str) or "<entry>" not in text:
        return text
    head, rest = text.split("<entry>", 1)
    parts = rest.split("<entry>")
    entries, tail = [], ""
    for i, part in enumerate(parts):
        content, sep, after = part.partition("</entry>")
        if i == len(parts) - 1:
            tail = after
        entries.append(content)
    entries.sort()
    return head + "".join(f"<entry>{e}</entry>" for e in entries) + tail


def drop_keys(obj, keys):
    """Recursively removes keys from a JSON value (for accepted per-endpoint differences)."""
    if isinstance(obj, dict):
        return {k: drop_keys(v, keys) for k, v in obj.items() if k not in keys}
    if isinstance(obj, list):
        return [drop_keys(v, keys) for v in obj]
    return obj


def zip_structure(data):
    """Structural fingerprint of a zip archive: (name, crc, method) per entry, in order.

    Byte equality is impossible across two servers (entry timestamps are the generation
    time); the content and layout are what must match."""
    import io
    import zipfile
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        return [(i.filename, i.CRC, i.compress_type) for i in z.infolist()]


# ---------------------------------------------------------------------------
# servers
# ---------------------------------------------------------------------------

def wait_ready(url, timeout=60):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2) as r:
                if r.status in (200, 401, 403):
                    return True
        except Exception:
            pass
        time.sleep(0.5)
    return False


def wait_tasks_done(base, auth, timeout=120):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            req = urllib.request.Request(base + "/api/v1/series?unpaged=true", headers=auth)
            with urllib.request.urlopen(req, timeout=3) as r:
                body = json.loads(r.read())
                if body.get("totalElements", 0) > 0:
                    return True
        except Exception:
            pass
        time.sleep(1)
    return False


def http(method, url, headers=None, body=None, raw=False):
    data = None
    if body is not None:
        data = body if isinstance(body, (bytes, str)) else json.dumps(body).encode()
        if isinstance(data, str):
            data = data.encode()
    req = urllib.request.Request(url, data=data, method=method)
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            content = r.read()
            return r.status, {k.lower(): v for k, v in r.headers.items()}, content
    except urllib.error.HTTPError as e:
        return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read()


def basic(email, password):
    import base64
    return {"Authorization": "Basic " + base64.b64encode(f"{email}:{password}".encode()).decode()}


# ---------------------------------------------------------------------------
# endpoint list
# ---------------------------------------------------------------------------

def endpoints():
    """(name, method, path, json_body, compare_body)

    compare_body: "json" | "bytes" | "jpeg" | "none"
    """
    E = []

    def get(name, path, compare="json"):
        E.append((name, "GET", path, None, compare))

    def post(name, path, body, compare="json"):
        E.append((name, "POST", path, body, compare))

    def delete(name, path, compare="json"):
        E.append((name, "DELETE", path, None, compare))

    # libraries / series / books read-only
    get("libraries", "/api/v1/libraries")
    get("library detail", "/api/v1/libraries/{LIB}")
    get("series list", "/api/v1/series?unpaged=true")
    get("series list paged", "/api/v1/series?page=0&size=1")
    post("series list post", "/api/v1/series/list", {"condition": {"publisher": {"operator": "is", "value": "Hakusensha"}}})
    get("series latest", "/api/v1/series/latest")
    get("series new", "/api/v1/series/new")
    get("series updated", "/api/v1/series/updated")
    get("series detail", "/api/v1/series/{SERIES_BERSERK}")
    get("series books", "/api/v1/series/{SERIES_BERSERK}/books")
    get("series collections", "/api/v1/series/{SERIES_BERSERK}/collections")
    get("series alphabetical", "/api/v1/series/alphabetical-groups")
    get("books list", "/api/v1/books?unpaged=true")
    get("books latest", "/api/v1/books/latest")
    get("books ondeck", "/api/v1/books/ondeck")
    get("books duplicates", "/api/v1/books/duplicates")
    get("book detail", "/api/v1/books/{BOOK_V01}")
    get("book pages", "/api/v1/books/{BOOK_V01}/pages")
    get("book page 1", "/api/v1/books/{BOOK_V01}/pages/1", "bytes")
    get("book page convert", "/api/v1/books/{BOOK_V01}/pages/1?convert=jpeg", "jpeg")
    get("book page thumbnail", "/api/v1/books/{BOOK_V01}/pages/1/thumbnail", "jpeg")
    get("book readlists", "/api/v1/books/{BOOK_V01}/readlists")
    get("book previous", "/api/v1/books/{BOOK_V01}/previous")
    get("book next", "/api/v1/books/{BOOK_V01}/next")
    get("book thumbnail", "/api/v1/books/{BOOK_V01}/thumbnail", "jpeg")
    get("book thumbnails", "/api/v1/books/{BOOK_V01}/thumbnails")
    get("book positions", "/api/v1/books/{BOOK_EPUB}/positions")
    get("book progression", "/api/v1/books/{BOOK_EPUB}/progression")
    get("book manifest divina", "/api/v1/books/{BOOK_V01}/manifest/divina")
    get("book manifest epub", "/api/v1/books/{BOOK_EPUB}/manifest/epub")
    get("book file", "/api/v1/books/{BOOK_V01}/file", "bytes")
    get("series thumbnail", "/api/v1/series/{SERIES_BERSERK}/thumbnail", "jpeg")
    get("series thumbnails", "/api/v1/series/{SERIES_BERSERK}/thumbnails")
    get("series tachiyomi", "/api/v2/series/{SERIES_BERSERK}/read-progress/tachiyomi")
    get("series zip", "/api/v1/series/{SERIES_BERSERK}/file", "zip")

    # collections / readlists
    get("collections", "/api/v1/collections")
    get("collection detail", "/api/v1/collections/{COLL}")
    get("collection series", "/api/v1/collections/{COLL}/series")
    get("collection thumbnail", "/api/v1/collections/{COLL}/thumbnail", "jpeg")
    get("readlists", "/api/v1/readlists")
    get("readlist detail", "/api/v1/readlists/{RL}")
    get("readlist books", "/api/v1/readlists/{RL}/books")
    get("readlist thumbnail", "/api/v1/readlists/{RL}/thumbnail", "jpeg")
    get("readlist zip", "/api/v1/readlists/{RL}/file", "zip")
    get("readlist tachiyomi", "/api/v1/readlists/{RL}/read-progress/tachiyomi")

    # referential
    get("v1 authors", "/api/v1/authors")
    get("v1 authors names", "/api/v1/authors/names")
    get("v1 authors roles", "/api/v1/authors/roles")
    get("v1 genres", "/api/v1/genres")
    get("v1 tags", "/api/v1/tags")
    get("v1 tags book", "/api/v1/tags/book")
    get("v1 tags series", "/api/v1/tags/series")
    get("v1 languages", "/api/v1/languages")
    get("v1 publishers", "/api/v1/publishers")
    get("v1 age-ratings", "/api/v1/age-ratings")
    get("v1 sharing-labels", "/api/v1/sharing-labels")
    get("v1 release-dates", "/api/v1/series/release-dates")
    get("v2 authors", "/api/v2/authors")
    get("v2 authors roles", "/api/v2/authors/roles")
    get("v2 authors names", "/api/v2/authors/names")
    get("v2 genres", "/api/v2/genres")
    get("v2 tags", "/api/v2/tags")
    get("v2 languages", "/api/v2/languages")
    get("v2 publishers", "/api/v2/publishers")
    get("v2 sharing-labels", "/api/v2/sharing-labels")
    get("v2 age-ratings", "/api/v2/age-ratings")
    get("v2 release-years", "/api/v2/series/release-years")

    # opds
    get("opds v1.2 catalog", "/opds/v1.2/catalog")
    get("opds v1.2 search", "/opds/v1.2/search")
    get("opds v1.2 ondeck", "/opds/v1.2/ondeck")
    get("opds v1.2 keep-reading", "/opds/v1.2/keep-reading")
    get("opds v1.2 series", "/opds/v1.2/series")
    get("opds v1.2 series latest", "/opds/v1.2/series/latest")
    get("opds v1.2 books latest", "/opds/v1.2/books/latest")
    get("opds v1.2 libraries", "/opds/v1.2/libraries")
    get("opds v1.2 collections", "/opds/v1.2/collections")
    get("opds v1.2 readlists", "/opds/v1.2/readlists")
    get("opds v1.2 publishers", "/opds/v1.2/publishers")
    get("opds v1.2 series detail", "/opds/v1.2/series/{SERIES_BERSERK}")
    get("opds v1.2 library detail", "/opds/v1.2/libraries/{LIB}")
    get("opds v2 auth", "/opds/v2/auth")
    get("opds v2 catalog", "/opds/v2/catalog")
    get("opds v2 keep-reading", "/opds/v2/libraries/keep-reading")
    get("opds v2 on-deck", "/opds/v2/libraries/on-deck")
    get("opds v2 books latest", "/opds/v2/libraries/books/latest")
    get("opds v2 series latest", "/opds/v2/libraries/series/latest")
    get("opds v2 browse", "/opds/v2/libraries/browse")
    get("opds v2 collections", "/opds/v2/libraries/collections")
    get("opds v2 readlists", "/opds/v2/libraries/readlists")
    get("opds v2 series detail", "/opds/v2/series/{SERIES_BERSERK}")
    get("opds v2 search", "/opds/v2/search?query=berserk")

    # misc
    get("settings", "/api/v1/settings")
    get("client-settings global", "/api/v1/client-settings/global/list")
    get("client-settings user", "/api/v1/client-settings/user/list")
    get("history", "/api/v1/history")
    get("fonts families", "/api/v1/fonts/families")
    get("page-hashes", "/api/v1/page-hashes")
    get("page-hashes unknown", "/api/v1/page-hashes/unknown")
    post("filesystem", "/api/v1/filesystem", {"path": "/tmp", "showFiles": False})
    get("actuator health", "/actuator/health")
    get("actuator info", "/actuator/info")
    get("actuator metrics", "/actuator/metrics")
    get("actuator scheduledtasks", "/actuator/scheduledtasks")
    get("users me", "/api/v2/users/me")
    delete("tasks empty", "/api/v1/tasks", "none")

    return E


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--java-jar")
    ap.add_argument("--java-home", default=os.environ.get("JAVA_HOME", ""))
    ap.add_argument("--rust-bin", default=os.path.expanduser("~/src/komga-rs/target/debug/komga-server"))
    ap.add_argument("--workdir", default="/tmp/komga-diff")
    ap.add_argument("--port-java", type=int, default=25611)
    ap.add_argument("--port-rust", type=int, default=25612)
    ap.add_argument("--skip-start", action="store_true")
    args = ap.parse_args()

    workdir = args.workdir
    shutil.rmtree(workdir, ignore_errors=True)
    os.makedirs(f"{workdir}/library", exist_ok=True)
    for src, dst in FIXTURES:
        os.makedirs(os.path.dirname(f"{workdir}/library/{dst}"), exist_ok=True)
        shutil.copy(os.path.join(KOMGA_RESOURCES, src), f"{workdir}/library/{dst}")

    java_dir = f"{workdir}/java"
    rust_dir = f"{workdir}/rust"
    os.makedirs(java_dir, exist_ok=True)
    os.makedirs(rust_dir, exist_ok=True)

    procs = []
    try:
        if not args.skip_start:
            java_bin = os.path.join(args.java_home, "bin", "java") if args.java_home else "java"
            print("[start] java komga ...")
            p = subprocess.Popen(
                [java_bin, "-jar", args.java_jar,
                 f"--server.port={args.port_java}",
                 f"--komga.config-dir={java_dir}",
                 "--spring.profiles.active=localdb,nogc"],
                stdout=open(f"{workdir}/java.log", "w"), stderr=subprocess.STDOUT)
            procs.append(p)

            print("[start] komga-rs ...")
            env = dict(os.environ, KOMGA_CONFIG_DIR=rust_dir, SERVER_PORT=str(args.port_rust))
            p = subprocess.Popen([args.rust_bin],
                                 stdout=open(f"{workdir}/rust.log", "w"), stderr=subprocess.STDOUT, env=env)
            procs.append(p)

        java_base = f"http://localhost:{args.port_java}"
        rust_base = f"http://localhost:{args.port_rust}"
        if not wait_ready(java_base + "/api/v1/claim", 120):
            print("java komga did not start in time"); sys.exit(2)
        if not wait_ready(rust_base + "/api/v1/claim", 30):
            print("komga-rs did not start in time"); sys.exit(2)

        auth = basic("admin@komga.org", "admin")
        for name, base in (("java", java_base), ("rust", rust_base)):
            s, _, _ = http("POST", base + "/api/v1/claim", headers={
                "X-Komga-Email": "admin@komga.org", "X-Komga-Password": "admin"})
            if s != 200:
                print(f"{name} claim failed: {s}"); sys.exit(2)
            s, _, _ = http("POST", base + "/api/v1/libraries", headers={**auth, "Content-Type": "application/json"},
                           body={"name": "Manga", "root": f"{workdir}/library"})
            if s != 200:
                print(f"{name} create library failed: {s}"); sys.exit(2)
            if not wait_tasks_done(base, auth):
                print(f"{name} initial scan did not finish"); sys.exit(2)
            time.sleep(3)  # let indexing and derived tasks settle

        # discover entity ids on each side (they differ across implementations)
        def get_json(base, path):
            s, _, body = http("GET", base + path, headers=auth)
            return json.loads(body) if s == 200 else None

        ids = {}
        aliases = {}
        for side, base in (("java", java_base), ("rust", rust_base)):
            lib = get_json(base, "/api/v1/libraries")[0]["id"]
            series = get_json(base, "/api/v1/series?unpaged=true")["content"]
            berserk = next(s for s in series if s["name"] == "berserk")
            books = get_json(base, f"/api/v1/series/{berserk['id']}/books")["content"]
            v01 = next(b for b in books if b["name"] == "v01")
            all_books = get_json(base, "/api/v1/books?unpaged=true")["content"]
            epub = next(b for b in all_books if b["name"] == "book")
            coll = get_json(base, "/api/v1/collections")
            rl = get_json(base, "/api/v1/readlists")
            ids[side] = {
                "LIB": lib,
                "SERIES_BERSERK": berserk["id"],
                "BOOK_V01": v01["id"],
                "BOOK_EPUB": epub["id"],
                "COLL": coll["content"][0]["id"] if coll.get("content") else None,
                "RL": rl["content"][0]["id"] if rl.get("content") else None,
            }
            # semantic aliases so TSIDs compare equal across implementations regardless of order
            alias = {}
            for library in get_json(base, "/api/v1/libraries"):
                alias[library["id"]] = f"LIB:{library['name']}"
            for s in series:
                alias[s["id"]] = f"SERIES:{s['name']}"
            for b in all_books:
                alias[b["id"]] = f"BOOK:{b['seriesTitle']}/{b['name']}"
            for c in coll.get("content", []):
                alias[c["id"]] = f"COLL:{c['name']}"
            for r in rl.get("content", []):
                alias[r["id"]] = f"RL:{r['name']}"
            for u in get_json(base, "/api/v2/users") or []:
                alias[u["id"]] = f"USER:{u['email']}"
            aliases[side] = alias

        mismatches = []
        total = 0
        for name, method, path, body, compare in endpoints():
            total += 1
            results = {}
            skip = False
            for side, base in (("java", java_base), ("rust", rust_base)):
                p = path
                for key, value in ids[side].items():
                    if value is None and "{" + key + "}" in p:
                        skip = True
                    p = p.replace("{" + key + "}", str(value))
                if skip:
                    break
                headers = dict(auth)
                if body is not None:
                    headers["Content-Type"] = "application/json"
                status, headers_, content = http(method, base + p, headers=headers, body=body)
                results[side] = (status, headers_, content)
            if skip:
                continue

            js, jh, jb = results["java"]
            rs, rh, rb = results["rust"]
            problems = []
            if js != rs:
                problems.append(f"status {js} != {rs}")
            else:
                if compare == "json":
                    rule = ACTUATOR_RULES.get(path)
                    if rule:
                        try:
                            problems.extend(rule(json.loads(jb), json.loads(rb)))
                        except json.JSONDecodeError as e:
                            problems.append(f"json parse failed: {e}")
                    else:
                        nj = normalize_body(jb, aliases["java"])
                        nr = normalize_body(rb, aliases["rust"])
                        if path.startswith(UNORDERED_FEED_PREFIXES):
                            nj = sort_feed_lists(nj)
                            nr = sort_feed_lists(nr)
                            nj = sort_xml_entries(nj)
                            nr = sort_xml_entries(nr)
                        if path.endswith("/thumbnails"):
                            # JPEG re-encoding differs across encoders (4:2:0 vs 4:4:4),
                            # so thumbnail byte sizes are never equal
                            nj = drop_keys(nj, {"fileSize"})
                            nr = drop_keys(nr, {"fileSize"})
                        if nj != nr:
                            problems.append("json body differs")
                elif compare == "bytes":
                    if jb != rb:
                        problems.append(f"bytes differ ({len(jb)} vs {len(rb)})")
                elif compare == "zip":
                    if zip_structure(jb) != zip_structure(rb):
                        problems.append("zip structure differs")
                elif compare == "jpeg":
                    if not (jb.startswith(b"\xff\xd8\xff") and rb.startswith(b"\xff\xd8\xff")):
                        problems.append("not both jpeg")
                # compare == "none": body not compared
                for header in ("content-type", "cache-control", "www-authenticate", "link", "content-disposition"):
                    jv, rv = jh.get(header), rh.get(header)
                    if (jv is None) != (rv is None):
                        problems.append(f"header {header} presence differs ({jv!r} vs {rv!r})")
                    elif jv is not None and jv != rv:
                        problems.append(f"header {header} differs ({jv!r} vs {rv!r})")
            if problems:
                mismatches.append((name, problems, jb, rb))

        outdir = f"{workdir}/out"
        shutil.rmtree(outdir, ignore_errors=True)
        os.makedirs(outdir, exist_ok=True)
        print(f"\n== {total - len(mismatches)}/{total} endpoints match ==")
        for name, problems, jb, rb in mismatches:
            print(f"\n--- DIFF: {name}")
            for p in problems:
                print(f"    {p}")
            slug = name.replace(" ", "_").replace("/", "_")
            # dump normalized bodies for offline inspection (what the comparison actually saw)
            for side, body in (("java", jb), ("rust", rb)):
                norm = normalize_body(body, aliases[side])
                if path.startswith(UNORDERED_FEED_PREFIXES):
                    norm = sort_xml_entries(sort_feed_lists(norm))
                if path.endswith("/thumbnails"):
                    norm = drop_keys(norm, {"fileSize"})
                if not isinstance(norm, str):
                    norm = json.dumps(norm, indent=2, sort_keys=True, ensure_ascii=False)
                with open(f"{outdir}/{slug}.{side}.txt", "w") as f:
                    f.write(norm)
            print(f"    dumped: {outdir}/{slug}.{{java,rust}}.txt")

        sys.exit(1 if mismatches else 0)
    finally:
        for p in procs:
            try:
                p.send_signal(signal.SIGINT)
            except Exception:
                pass
        for p in procs:
            try:
                p.wait(timeout=15)
            except Exception:
                p.kill()


if __name__ == "__main__":
    main()
