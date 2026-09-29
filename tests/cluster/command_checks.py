"""Trusted application fixture on independent command stores; no production auth."""
from concurrent.futures import ThreadPoolExecutor
import json
import socket
import sys
from checks import IDS, ROOT, PROJECT, command, compose, container, poll, request

ACK = ROOT / 'commands-acknowledged.json'


def api(node, action, body=None):
    return request(node, f'commands-{action}', body)


def leader(ids=IDS):
    def find():
        return next((i for i in ids if api(i, 'ready')[0] == 200), None)
    return poll('application command quorum', find)


def payload(target='item', key='first', expected=0, tenant='tenant-a', principal='alice'):
    return dict(target=target, key=key, expected=expected, tenant=tenant, principal=principal)


def submit(node, body):
    code, result = api(node, 'submit', body)
    assert code == 200, (code, result, body)
    return result


def state(node):
    code, body = api(node, 'state')
    assert code == 200 and not body['failed'], (code, body)
    return body


def counts(node):
    code, body = api(node, 'counts')
    assert code == 200, (code, body)
    return body


def save(first=None):
    previous = json.loads(ACK.read_text()) if ACK.exists() else {}
    previous['counts'] = counts(leader())
    if first is not None:
        previous['first'] = first
    ACK.write_text(json.dumps(previous, sort_keys=True))


def converge():
    expected = json.loads(ACK.read_text())
    def check():
        node = leader()
        assert counts(node) == expected['counts']
        assert submit(node, payload()) == expected['first']
        return len({state(i)['data_sha256'] for i in IDS}) == 1
    poll('application data, events, outbox and receipts converge', check)


def atomic():
    for i in IDS:
        assert not state(i)['initialized']
        assert api(i, 'ready')[0] == 503
        try:
            with socket.create_connection((f'node{i}', 9653), timeout=2):
                raise AssertionError('command peer listener exposed on control network')
        except (ConnectionRefusedError, TimeoutError):
            pass
    assert api(1, 'bootstrap', {})[0] == 200
    node = leader()
    first = submit(node, payload())
    assert submit(node, payload()) == first
    assert api(node, 'submit', payload(target='conflict'))[0] != 200
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(lambda key: api(node, 'submit', payload(key=key, expected=1)), ['left', 'right']))
    assert sorted(code for code, _ in results) == [200, 503], results
    other = submit(node, payload(tenant='tenant-b'))
    assert other['id'] != first['id']
    assert counts(node) == dict(tasks=2, operations=3, events=3, outbox=3, receipts=3)
    policy = dict(tenant='tenant-a', principal='alice', allowed=False)
    assert api(node, 'permission', policy)[0] == 200
    assert api(node, 'submit', payload())[0] != 200
    # Another principal's receipt cannot be disclosed through the same key.
    assert api(node, 'submit', payload(principal='eve'))[0] != 200
    policy['allowed'] = True
    assert api(node, 'permission', policy)[0] == 200
    save(first)
    converge()


def retention():
    node = leader()
    lagging = next(i for i in IDS if i != node)
    before = state(lagging)['applied']
    compose('kill', '-s', 'SIGKILL', f'node{lagging}')
    for n in range(265):
        body = payload(target=f'later-{n}', key=f'later-{n}')
        poll(f'command {n} acknowledged', lambda: submit(leader(tuple(i for i in IDS if i != lagging)), body))
    node = leader(tuple(i for i in IDS if i != lagging))
    assert submit(node, payload()) == json.loads(ACK.read_text())['first']
    poll('application log compacted beyond missing follower', lambda: (state(node)['purged'] or 0) > before)
    compose('start', f'node{lagging}')
    poll('application snapshot installed', lambda: state(lagging)['snapshot_installs'] > 0)
    save()
    assert json.loads(ACK.read_text())['counts'] == dict(tasks=267, operations=268, events=268, outbox=268, receipts=268)
    converge()


def failover():
    node = leader()
    body = json.dumps(payload(target='lost', key='lost-response')).encode()
    before = counts(node)['operations']
    with socket.create_connection((f'node{node}', 8080), timeout=3) as stream:
        stream.sendall(b'POST /test/commands-submit HTTP/1.1\r\nHost: fixture\r\nContent-Type: application/json\r\nContent-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
        poll('application committed with response unread', lambda: counts(node)['operations'] == before + 1)
    first_reply = submit(node, json.loads(body))
    save()
    compose('kill', '-s', 'SIGKILL', f'node{node}')
    elected = leader(tuple(i for i in IDS if i != node))
    assert submit(elected, json.loads(body)) == first_reply
    assert counts(elected) == json.loads(ACK.read_text())['counts']
    compose('start', f'node{node}')
    converge()


def partition():
    victim = leader()
    peer = container(victim)
    network = f'{PROJECT}-replication'
    address = json.loads(command('docker', 'inspect', peer))[0]['NetworkSettings']['Networks'][network]['IPAddress']
    command('docker', 'network', 'disconnect', network, peer)
    try:
        elected = leader(tuple(i for i in IDS if i != victim))
        assert api(victim, 'ready')[0] == 503
        assert api(victim, 'counts')[0] == 503
        assert api(victim, 'submit', payload(target='unsafe', key='minority'))[0] == 503
        submit(elected, payload(target='majority', key='majority'))
        expected = json.loads(ACK.read_text())
        expected['counts'] = counts(elected)
        ACK.write_text(json.dumps(expected))
    finally:
        command('docker', 'network', 'connect', '--ip', address, '--alias', f'raft{victim}', network, peer)
    converge()


def restart():
    compose('kill', '-s', 'SIGKILL', 'node1', 'node2', 'node3')
    compose('start', 'node1', 'node2', 'node3')
    leader()
    converge()
    for i in IDS:
        assert api(i, 'bootstrap', {})[0] == 503


if __name__ == '__main__':
    {'atomic': atomic, 'retention': retention, 'failover': failover,
     'partition': partition, 'restart': restart}[sys.argv[1]]()
    print(f'PASS application commands {sys.argv[1]}: {ACK.read_text()}')
