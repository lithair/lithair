"""Actual generated Lithair HTTP routes; the control plane only operates nodes."""
from concurrent.futures import ThreadPoolExecutor
import json
import socket
import sys
import urllib.error
import urllib.request
from checks import IDS, ROOT, PROJECT, HTTP, command, compose, container, poll, request

ACK = ROOT / 'native-acknowledged.json'


def api(node, path='/api/records', method='GET', body=None, key=None):
    headers = {'Content-Type': 'application/json'}
    if key is not None:
        headers['Idempotency-Key'] = key
    req = urllib.request.Request(f'http://node{node}:8180{path}', method=method,
                                 headers=headers, data=None if body is None else json.dumps(body).encode())
    try:
        response = HTTP.open(req, timeout=8)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        assert 'Location' not in response.headers, 'private leader redirect'
        return response.code, json.loads(raw) if raw else None


def leader(ids=IDS):
    def find():
        for node in ids:
            if api(node, '/ready')[0] == 200:
                return node
        return None
    return poll('native application leader has a current quorum', find)


def report(node):
    code, body = request(node, 'native-state')
    assert code == 200 and not body['failed'], (code, body)
    return body


def save(value):
    ACK.write_text(json.dumps(value, sort_keys=True))


def converge():
    acknowledged = json.loads(ACK.read_text())
    def check():
        node = leader()
        code, records = api(node)
        assert code == 200, (code, records)
        actual = {r['id']: r for r in records['data']}
        assert actual == acknowledged, (actual, acknowledged)
        states = [report(i) for i in IDS]
        return len({s['data_sha256'] for s in states}) == 1
    poll('native replicas preserve all acknowledged records', check)


def create(node, identity):
    body = {'id': identity, 'email': identity, 'name': identity}
    code, value = api(node, method='POST', body=body, key=f'create-{identity}')
    assert code == 201, (code, value)
    return value


def crud():
    poll('native public listeners started', lambda: all(api(i, '/health')[0] == 200 for i in IDS))
    for i in IDS:
        assert not report(i)['initialized']
        assert api(i, '/ready')[0] == 503
        assert api(i, '/_admin')[0] == api(i, '/_admin/data/models')[0] == 404
        assert api(i, '/raft/v2/append', 'POST', {}, 'untrusted')[0] == 404
        try:
            with socket.create_connection((f'node{i}', 9553), timeout=2):
                raise AssertionError('native mTLS peer listener is reachable on public/control network')
        except (ConnectionRefusedError, TimeoutError):
            pass
    assert request(1, 'native-bootstrap', {})[0] == 200
    node = leader()
    with ThreadPoolExecutor(max_workers=8) as pool:
        records = list(pool.map(lambda n: create(node, f'concurrent-{n}'), range(12)))
    main = create(node, 'main')
    with ThreadPoolExecutor(max_workers=2) as pool:
        replies = list(pool.map(lambda pair: api(node, '/api/records/main', 'PATCH', {pair[0]: pair[1]}, pair[0]), [('left', 7), ('right', 9)]))
    assert all(code == 200 for code, _ in replies), replies
    code, main = api(node, '/api/records/main')
    assert code == 200 and main['left'] == 7 and main['right'] == 9, main
    assert api(node, '/api/records/main', 'PATCH', {'id': 'changed'}, 'pk-change')[0] == 400
    assert api(node, '/api/records/main', 'PATCH', {'name': ''}, 'invalid')[0] == 400
    assert api(node, method='POST', body={'id': 'collision', 'email': 'main', 'name': 'x'}, key='collision')[0] == 409
    follower = next(i for i in IDS if i != node)
    assert api(follower)[0] == 503
    assert api(follower, '/api/records/main', 'PATCH', {'name': 'unsafe'}, 'follower')[0] == 503
    # Discard the response on a real connection. Observe commitment through a
    # separate read before closing: an earlier disconnect might precede admission
    # entirely and would not prove response-loss retry behavior.
    body = json.dumps({'id': 'lost', 'email': 'lost', 'name': 'lost'}).encode()
    with socket.create_connection((f'node{node}', 8180), timeout=3) as stream:
        stream.sendall(b'POST /api/records HTTP/1.1\r\nHost: app\r\nContent-Type: application/json\r\nIdempotency-Key: lost-response\r\nContent-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
        poll('unread-response request committed', lambda: api(node, '/api/records/lost')[0] == 200)
    code, lost = api(node, method='POST', body=json.loads(body), key='lost-response')
    assert code == 201, (code, lost)
    assert api(node, method='POST', body={'id': 'other'}, key='lost-response')[0] == 409
    create(node, 'deleted')
    assert api(node, '/api/records/deleted', 'DELETE', key='delete')[0] == 204
    assert api(node, '/api/records/deleted', 'DELETE', key='delete') == (204, None)
    save({r['id']: r for r in records + [main, lost]})
    converge()


def recovery():
    victim = leader()
    previous = report(victim)['applied']
    compose('kill', '-s', 'SIGKILL', f'node{victim}')
    survivors = tuple(i for i in IDS if i != victim)
    node = leader(survivors)
    acknowledged = json.loads(ACK.read_text())
    for index in range(40):
        item = create(node, f'after-kill-{index}')
        acknowledged[item['id']] = item
        save(acknowledged)
    poll('native leader purged stopped replica prefix', lambda: (report(node)['purged'] or 0) > previous)
    compose('start', f'node{victim}')
    poll('native follower installed real application snapshot', lambda: report(victim)['snapshot_installs'] > 0)
    converge()


def partition():
    victim = leader()
    peer = container(victim)
    network = f'{PROJECT}-replication'
    info = json.loads(command('docker', 'inspect', peer))[0]
    address = info['NetworkSettings']['Networks'][network]['IPAddress']
    command('docker', 'network', 'disconnect', network, peer)
    try:
        node = leader(tuple(i for i in IDS if i != victim))
        assert api(victim, '/ready')[0] == 503
        assert api(victim, '/api/records/main')[0] == 503
        assert api(victim, '/api/records/main', 'PATCH', {'left': 999}, 'minority')[0] == 503
        item = create(node, 'majority-native')
        acknowledged = json.loads(ACK.read_text())
        acknowledged[item['id']] = item
        save(acknowledged)
    finally:
        command('docker', 'network', 'connect', '--ip', address, '--alias', f'raft{victim}', network, peer)
    converge()


def restart():
    compose('kill', '-s', 'SIGKILL', 'node1', 'node2', 'node3')
    compose('start', 'node1', 'node2', 'node3')
    node = leader()
    converge()
    assert api(node, '/api/records/deleted')[0] == 404
    assert api(node, '/api/records/deleted', 'DELETE', key='delete') == (204, None)
    code, lost = api(node, method='POST', body={'id': 'lost', 'email': 'lost', 'name': 'lost'}, key='lost-response')
    assert code == 201 and lost == json.loads(ACK.read_text())['lost']
    for i in IDS:
        assert request(i, 'native-bootstrap', {})[0] == 503
    converge()


if __name__ == '__main__':
    {'crud': crud, 'recovery': recovery, 'partition': partition, 'restart': restart}[sys.argv[1]]()
    print(f'PASS native {sys.argv[1]}: {len(json.loads(ACK.read_text()))} acknowledged records')
