"""Black-box test driver. Only the test HTTP API and Docker lifecycle are used."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

IDS = (1, 2, 3)
ROOT = Path('/evidence')
STATE = ROOT / 'acknowledged.json'
PROJECT = os.environ['COMPOSE_PROJECT_NAME']
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def command(*args, timeout=30):
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'{args!r} exited {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout.strip()


def compose(*args, timeout=30):
    return command('docker', 'compose', '-f', '/suite/compose.yml', '-p', PROJECT, *args, timeout=timeout)


def request(node, path='state', body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(f'http://node{node}:8080/test/{path}', data=data,
                                 headers={'Content-Type': 'application/json'})
    try:
        with HTTP.open(req, timeout=12) as response:
            return response.status, json.load(response)
    except urllib.error.HTTPError as error:
        return error.code, json.load(error)


def state(node):
    code, value = request(node)
    assert code == 200 and value['id'] == node and value['running'], (node, code, value)
    return value


def poll(description, predicate, seconds=40):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (AssertionError, OSError, urllib.error.URLError) as error:
            last = repr(error)
        time.sleep(0.1)  # bounded condition polling, never a readiness assertion by sleep
    raise AssertionError(f'deadline: {description}; last observation: {last}')


def ready(ids=IDS):
    return poll('nodes answering their own IDs', lambda: all(state(i) for i in ids))


def leader(ids=IDS):
    def find():
        for node in ids:
            value = state(node)
            if value['leader'] == node and request(node, 'barrier', {})[0] == 200:
                return node
        return None
    return poll('leader with successful consistent-read barrier', find)


def expected():
    return json.loads(STATE.read_text()) if STATE.exists() else {}


def write_batch(prefix, count, ids=IDS):
    for index in range(count):
        key, value = f'{prefix}-{index}', f'value-{prefix}-{index}'
        def write():
            target = leader(ids)
            return request(target, 'write', {'key': key, 'value': value})[0] == 200
        poll(f'acknowledge {key}', write)
        acknowledged = expected()
        acknowledged[key] = value
        STATE.write_text(json.dumps(acknowledged))


def converge():
    acknowledged = expected()
    def equal():
        values = [state(i)['state'] for i in IDS]
        assert all(all(value.get(k) == v for k, v in acknowledged.items()) for value in values), values
        return all(value == values[0] for value in values)
    poll('all replicas preserve every acknowledged mutation and converge', equal)


def container(node):
    result = compose('ps', '-a', '-q', f'node{node}')
    assert result and '\n' not in result, result
    return result


def pristine():
    ready()
    for node in IDS:
        value = state(node)
        assert not value['initialized'] and value['state'] == {}, value
    if os.environ.get('LITHAIR_CLUSTER_INJECT_FAILURE') == '1':
        raise AssertionError('deliberate outer-gate failure to verify cleanup and exit propagation')


def isolation():
    data_volumes, key_volumes = set(), set()
    for node in IDS:
        info = json.loads(command('docker', 'inspect', container(node)))[0]
        assert not info['HostConfig']['PortBindings'], 'fixture must publish no host ports'
        for mount in info['Mounts']:
            if mount['Destination'] == '/data':
                data_volumes.add(mount['Name'])
            if mount['Destination'] == '/secrets':
                assert not mount['RW'], 'node credentials must be read-only'
                key_volumes.add(mount['Name'])
        # On the control network DNS resolves nodeN to its control interface.
        try:
            with socket.create_connection((f'node{node}', 9443), timeout=2):
                raise AssertionError('peer listener is reachable on the control plane')
        except (ConnectionRefusedError, TimeoutError):
            pass
        try:
            HTTP.open(urllib.request.Request(f'http://node{node}:8080/raft/v2/preflight',
                      data=b'{}', headers={'Content-Type': 'application/json'}), timeout=2)
            raise AssertionError('Raft route exposed on test control listener')
        except urllib.error.HTTPError as error:
            assert error.code == 404
    assert len(data_volumes) == len(key_volumes) == 3
    result = compose('exec', '-T', 'node2', '/usr/local/bin/cluster-compose-node',
                     'reject-unauthenticated', '1')
    assert 'rejected the client without a certificate' in result


def bootstrap():
    compose('stop', 'node3')
    assert request(1, 'initialize', {})[0] == 503
    assert not state(1)['initialized'] and not state(2)['initialized']
    compose('start', 'node3')
    ready()
    assert request(1, 'initialize', {}) == (200, {'ok': True})
    leader()
    write_batch('initial', 12)
    converge()


def failover():
    victim = leader()
    compose('kill', '-s', 'SIGKILL', f'node{victim}')
    victim_container = container(victim)
    def killed():
        info = json.loads(command('docker', 'inspect', victim_container))[0]['State']
        return not info['Running'] and info['ExitCode'] == 137
    poll('killed leader exited with SIGKILL', killed)
    survivors = tuple(i for i in IDS if i != victim)
    write_batch('after-kill', 4, survivors)
    compose('start', f'node{victim}')
    ready()
    converge()


def partition():
    victim = leader()
    node_container = container(victim)
    network = f'{PROJECT}_replication'
    info = json.loads(command('docker', 'inspect', node_container))[0]
    address = info['NetworkSettings']['Networks'][network]['IPAddress']
    command('docker', 'network', 'disconnect', network, node_container)
    try:
        assert network not in json.loads(command('docker', 'inspect', node_container))[0]['NetworkSettings']['Networks']
        survivors = tuple(i for i in IDS if i != victim)
        leader(survivors)
        assert request(victim, 'write', {'key': 'uncertain-minority-write', 'value': 'not-acknowledged'})[0] == 503
        assert request(victim, 'barrier', {})[0] == 503
        write_batch('majority', 4, survivors)
    finally:
        # Preserve the address already enrolled in the running transports.
        command('docker', 'network', 'connect', '--ip', address, '--alias', f'raft{victim}', network, node_container)
    converge()


def snapshot():
    victim = next(i for i in IDS if i != leader())
    previous = state(victim)['applied']
    compose('kill', '-s', 'SIGKILL', f'node{victim}')
    survivors = tuple(i for i in IDS if i != victim)
    write_batch('snapshot-suffix', 32, survivors)
    def purged():
        current = state(leader(survivors))['purged']
        return current is not None and current > previous
    poll('leader durably purged the stopped follower prefix', purged)
    compose('start', f'node{victim}')
    ready()
    poll('follower actually installed a remote snapshot', lambda: state(victim)['installed'] > 0)
    converge()


def restart():
    compose('kill', '-s', 'SIGKILL', 'node1', 'node2', 'node3')
    compose('start', 'node1', 'node2', 'node3')
    ready()
    leader()
    converge()
    for node in IDS:
        assert state(node)['initialized']
        assert request(node, 'initialize', {})[0] == 503


def negative():
    directory = ROOT / 'negative'
    directory.mkdir()
    config = directory / 'probatum.toml'
    config.write_text('[[check]]\nget = "http://node1:8080/test/state"\ncontains = ["INTENTIONALLY_ABSENT_VALUE_261"]\n')
    result = subprocess.run(['probatum', 'run', str(config), '--json'], cwd=directory,
                            capture_output=True, text=True, timeout=15)
    (directory / 'verdict.json').write_text(result.stdout)
    (directory / 'stderr.log').write_text(result.stderr)
    assert result.returncode == 1, (result.returncode, result.stdout, result.stderr)
    assert isinstance(json.loads(result.stdout), dict), 'missing structured failure evidence'


def inspect():
    compose('stop', 'node1', 'node2', 'node3')
    for node in IDS:
        result = compose('run', '--rm', '-T', '--no-deps', f'node{node}', 'inspect', str(node))
        value = json.loads(result)
        assert value['scope'] == 'offline_consensus_integrity' and not value['pristine']
        assert value['bootstrap_claimed'] == (node == 1)
        (ROOT / f'node{node}-inspection.json').write_text(result)


if __name__ == '__main__':
    actions = {fn.__name__: fn for fn in (pristine, isolation, bootstrap, failover, partition, snapshot, restart, negative, inspect)}
    actions[sys.argv[1]]()
    print(f'PASS: {sys.argv[1]} ({len(expected())} acknowledged mutations retained)')
