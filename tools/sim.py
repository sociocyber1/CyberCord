#!/usr/bin/env python3
"""Simulate a CyberCord network on one machine and measure what users feel.

Every simulated user is a headless CyberCord with its own data folder, its own
address (127.0.x.y — a separate "IP"), local discovery (mDNS) off and the
global DHT off. Each one knows a single random earlier user, as if it had met
the network through the rendezvous once. Nobody adds friends.

Then it checks:
  1. user 1 types the handle of the newest user → found? how fast?
  2. user 1 searches the newest user by name;
  3. how many users each sampled user sees on the map (expected: all);
  4. searching specialists by skill;
  5. memory and CPU of the whole network.

usage: tools/sim.py N [--bin target/release/cybercord] [--settle 60]
"""
import argparse, json, os, random, re, shutil, subprocess, sys, time

FIRST = ["Анна", "Иван", "Мария", "Олег", "Лена", "Пётр", "Ольга", "Сергей", "Нина", "Павел", "Alice", "Bob", "Carol", "Dmitri", "Eva", "Fatima", "Goran", "Hana", "Ivo", "Jun",
         "Kofi", "Lucia", "Mateo", "Nadia", "Omar", "Priya", "Quinn", "Ravi", "Sofia", "Tariq"]
LAST = ["Соколов", "Иванова", "Ким", "Новак", "Silva", "Müller", "Tanaka", "Okafor", "Rossi", "Nguyen", "Kowalski", "Haddad", "Larsen", "Garcia", "Petrov"]
SKILLS = ["Ma", "Ph", "Ch", "Bi", "El", "En", "Ro", "GS", "Sc", "Cy", "Ag", "Pc", "Em", "Au"]
SKILL_QUERY = {"Ma": "Математика", "Ph": "Physics", "Ch": "Химия", "Bi": "Biology", "El": "Электроника", "En": "Engineering", "Ro": "Робототехника",
               "GS": "General semantics", "Sc": "Социокибернетика", "Cy": "Cybernetics", "Ag": "Atom-Generator", "Pc": "KiCad", "Em": "Embedded", "Au": "Автоматизация"}


def ip_of(i):
    return f"127.0.{1 + i // 250}.{2 + i % 250}"


class Node:
    def __init__(self, i, name, root, binary, boot):
        self.i, self.name = i, name
        self.dir = os.path.join(root, f"n{i:03}")
        os.makedirs(self.dir, exist_ok=True)
        self.log_path = os.path.join(self.dir, "out.log")
        self.fifo = os.path.join(self.dir, "in.fifo")
        os.mkfifo(self.fifo)
        env = dict(os.environ, CYBERCORD_DATA_DIR=self.dir, CYBERCORD_ALLOW_LOOPBACK="1", CYBERCORD_NO_GLOBAL="1", CYBERCORD_LISTEN_IP=ip_of(i), RUST_LOG="error", CYBERCORD_DEBUG_CONNS="1")
        args = [binary, "--headless", "--no-mdns", "--name", name]
        if boot:
            args += ["--bootstrap", boot]
        self.log = open(self.log_path, "w")
        # open the pipe read+write (raw, unbuffered) so neither side blocks
        fd = os.open(self.fifo, os.O_RDWR)
        self.proc = subprocess.Popen(args, stdin=fd, stdout=self.log, stderr=subprocess.STDOUT, env=env)
        self.inp = os.fdopen(fd, "w", buffering=1)
        self.id = None

    def text(self):
        with open(self.log_path, encoding="utf-8", errors="replace") as f:
            return f.read()

    def send(self, line):
        self.inp.write(line + "\n")
        self.inp.flush()

    def wait_for(self, pattern, timeout, after=0):
        t0 = time.time()
        while time.time() - t0 < timeout:
            m = re.search(pattern, self.text()[after:])
            if m:
                return m, time.time() - t0
            time.sleep(0.2)
        return None, timeout

    def addr(self):
        return f"/ip4/{ip_of(self.i)}/tcp/47100/p2p/{self.id}"


def rss_cpu(pids):
    rss, cpu = 0, 0.0
    out = subprocess.run(["ps", "-o", "rss=,pcpu=", "-p", ",".join(map(str, pids))], capture_output=True, text=True).stdout
    for line in out.split("\n"):
        p = line.split()
        if len(p) == 2:
            rss += int(p[0])
            cpu += float(p[1])
    return rss / 1024, cpu


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("n", type=int)
    ap.add_argument("--bin", default="target/release/cybercord")
    ap.add_argument("--root", default="/tmp/ccsim")
    ap.add_argument("--settle", type=int, default=60)
    ap.add_argument("--seed", type=int, default=7)
    a = ap.parse_args()
    random.seed(a.seed)
    shutil.rmtree(a.root, ignore_errors=True)
    os.makedirs(a.root)
    binary = os.path.abspath(a.bin)
    nodes = []
    report = {"n": a.n}
    t_start = time.time()
    for i in range(a.n):
        name = f"{random.choice(FIRST)} {random.choice(LAST)}"
        boot = random.choice(nodes).addr() if nodes else None
        nd = Node(i, name, a.root, binary, boot)
        m, _ = nd.wait_for(r"you are (\S+)", 20)
        if not m:
            print(f"node {i} did not start", file=sys.stderr)
            print(nd.text()[-2000:], file=sys.stderr)
            sys.exit(1)
        nd.id = m.group(1)
        nodes.append(nd)
        skills = random.sample(SKILLS, 2)
        nd.send(f"/profile {skills[0]}{random.randint(4, 9)} {skills[1]}{random.randint(3, 8)}")
        nd.skills = skills
        time.sleep(0.15)
    report["start_s"] = round(time.time() - t_start, 1)
    print(f"{a.n} users started in {report['start_s']} s; settling {a.settle} s…", flush=True)
    time.sleep(a.settle)

    pids = [nd.proc.pid for nd in nodes]
    alive = sum(1 for nd in nodes if nd.proc.poll() is None)
    report["alive"] = alive
    report["rss_mb"], report["cpu_pct"] = rss_cpu(pids)

    # 3. the map: how many users does each sampled user see?
    sample = [nodes[1], nodes[len(nodes) // 2], nodes[-1]]
    seen = {}
    for nd in sample:
        mark = len(nd.text())
        nd.send("/nodes")
        m, _ = nd.wait_for(r"(\d+) nodes known", 10, after=mark)
        seen[nd.name] = int(m.group(1)) if m else None
    report["map_sees"] = seen
    conns = []
    for nd in sample:
        c = re.findall(r"(\d+) known · (\d+) connected", nd.text())
        conns.append(int(c[-1][1]) if c else None)
    report["connections"] = conns

    # 1. handle search: user 1 → newest user (they share no friend, met through different people)
    a_node, b_node = nodes[1], nodes[-1]
    mark_b = len(b_node.text())
    b_node.send("/handle")
    m, _ = b_node.wait_for(r"your handle: (@\S+)", 10, after=mark_b)
    handle = m.group(1) if m else None
    report["target"] = {"name": b_node.name, "handle": handle}
    mark = len(a_node.text())
    a_node.send(f"/find {handle}")
    m, t = a_node.wait_for(r'search "' + re.escape(handle) + r'": [1-9]\d* found', 45, after=mark)
    report["handle_found"] = bool(m)
    report["handle_seconds"] = round(t, 1)
    found_via = re.findall(re.escape(b_node.id) + r"\s+\S+\s+\[(\w+)\]", a_node.text()[mark:])
    report["handle_via"] = found_via[-1] if found_via else None

    # 2. name search
    mark = len(a_node.text())
    a_node.send(f"/find {b_node.name.split()[0]}")
    m, t = a_node.wait_for(r'search "[^"]+": [1-9]\d* found', 30, after=mark)
    hits = re.findall(r"^\s{4}(.+?)\s{2,}(12D3\S+)", a_node.text()[mark:], re.M)
    report["name_search"] = {"found": bool(m), "seconds": round(t, 1), "target_in_results": any(p == b_node.id for _, p in hits)}

    # 4. specialists: anyone with skill X?
    want = nodes[-1].skills[0]
    mark = len(a_node.text())
    a_node.send(f"/find {SKILL_QUERY[want]}")
    m, t = a_node.wait_for(r'search "[^"]+": (\d+) found', 15, after=mark)
    experts = sum(1 for nd in nodes if want in nd.skills)
    report["skill_search"] = {"skill": SKILL_QUERY[want], "users_with_skill": experts, "found": int(m.group(1)) if m else 0}

    report["rss_mb_after"], report["cpu_pct_after"] = rss_cpu(pids)
    report["errors"] = sum(nd.text().count("panicked") for nd in nodes)
    print(json.dumps(report, ensure_ascii=False, indent=2))
    with open(os.path.join(a.root, "report.json"), "w") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)
    if not os.environ.get("SIM_KEEP"):
        for nd in nodes:
            nd.proc.terminate()


if __name__ == "__main__":
    main()
