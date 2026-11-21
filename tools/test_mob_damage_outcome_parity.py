#!/usr/bin/env python3
"""Compare live /damage results, health and motion for mob enchantment immunity."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess

try:
    from . import lab, oracle_harness, test_rust_parity as base
    from .integration_env import run_game
except ImportError:
    import lab
    import oracle_harness
    import test_rust_parity as base
    from integration_env import run_game

RECORDING = 'mob_damage_outcome'


def scene():
    ops = []
    fields = {}

    def command(cmd, field=None, result=None):
        ops.append({'op': 'cmd', 'cmd': '/' + cmd, 'record': field is not None})
        if field is not None:
            fields[ops[-1]['cmd']] = field, result

    command('tick freeze')
    command('gamerule spawn_mobs false')
    command('forceload add 0 0')
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    command('tp @s 14.5 100 14.5')
    command('fill 0 99 0 15 99 15 stone')
    boot = '{id:"minecraft:leather_boots",count:1,components:{' \
           '"minecraft:attribute_modifiers":[],"minecraft:enchantments":{"minecraft:frost_walker":1}}}'
    for n, (name, slot, damage, accepts) in enumerate([
        ('immune', 'feet', 'campfire', False),
        ('wrong_source', 'feet', 'in_fire', True),
        ('wrong_slot', 'mainhand', 'campfire', True),
    ], 1):
        uuid = f'000000cb-0000-0000-0000-{n:012x}'
        x = n * 3
        command(f'summon pig {x + .5} 100 5.5 {{UUID:[I;203,0,0,{n}],NoAI:1b,NoGravity:1b,'
                f'Health:10f,equipment:{{{slot}:{boot}}}}}', f'{name}/summon', True)
        command(f'execute if data entity {uuid} equipment.{slot}.components."minecraft:enchantments".'
                '"minecraft:frost_walker"', f'{name}/equipped', True)
        command(f'damage {uuid} 2 minecraft:{damage} at {x + 1.5} 100 5.5', f'{name}/hit', accepts)
        for key in ['Health', 'HurtTime', 'Motion[0]', 'Motion[1]', 'Motion[2]']:
            command(f'data get entity {uuid} {key}', f'{name}/{key}', True)
    return ops, fields


def observed(path, fields):
    records = [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()]
    if [row['cmd'] for row in records] != list(fields):
        raise ValueError('Incomplete or reordered mob damage recording')
    observations = {}
    for row in records:
        field, expected_success = fields[row['cmd']]
        result, = row['results']
        value = result
        if field.endswith(('/Health', '/HurtTime', '/Motion[0]', '/Motion[1]', '/Motion[2]')):
            # /data get prints the exact SNBT scalar, while the command result floors floats.
            message, = row['messages']
            scalar = ''.join(segment[0] for segment in message).rsplit(': ', 1)[-1]
            number, suffix = scalar[:-1], scalar[-1]
            if field.endswith('/Health') and suffix.lower() == 'f':
                value = struct.pack('>f', float(number)).hex()
            elif '/Motion[' in field and suffix.lower() == 'd':
                value = struct.pack('>d', float(number)).hex()
            elif field.endswith('/HurtTime') and suffix.lower() == 's':
                value = int(number)
            else:
                raise ValueError(f'Unexpected scalar type for {field}')
        if path.name == 'vanilla.jsonl' and result[0] != expected_success:
            raise ValueError(f'Vanilla fixture prerequisite failed: {field}')
        observations[field] = value
    return observations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / RECORDING)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--binary', type=Path, help='use an already built Sunrise game')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=False)
    ops, fields = scene()
    script = work / 'scene.jsonl'
    script.write_text(''.join(json.dumps(op) + '\n' for op in ops), encoding='utf-8')
    lock, jars, libraries = base.verify_artifacts()
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    classes = work / 'classes'
    oracle = lab.ROOT / 'mods/sunrise/src/test/java/dev/sunriselab/parity/VanillaCommandExecOracle.java'
    oracle_harness.run_logged(['javac', '--release', lock['minecraft']['java_major'], '-proc:none',
                               '-cp', dependencies, '-d', classes, oracle], work, 'javac')
    vanilla = work / 'vanilla.jsonl'
    oracle_harness.java_run(work, classes, dependencies, 'VanillaCommandExecOracle',
                            [work / 'universe', script, vanilla, '1'], 'java',
                            jvm=['-Dsunrise.parity.flat=true'])
    expected = observed(vanilla, fields)
    if args.reference_only:
        print(f'{RECORDING}: Minecraft {lock["minecraft"]["version"]} reference captured')
        return 0

    binary = args.binary
    if binary is None:
        oracle_harness.run_logged(['cargo', 'build', '--locked', '--profile', 'fast',
                                   '-p', 'sunrise-game', '--bin', 'sunrise-client'], work, 'build')
        binary = lab.binary(lab.cargo_target() / 'fast', 'sunrise-client')
    scratch = work / 'native'
    scratch.mkdir()
    native = work / 'native.jsonl'
    env = lab.game_env(SUNRISE_COMMAND_SCRIPT=script, SUNRISE_COMMAND_OUTPUT=native,
                       SUNRISE_PLAYER_NAME='Player', SUNRISE_API_DISCOVERY='http://127.0.0.1:9/')
    with (work / 'native.log').open('w', encoding='utf-8') as log:
        status = run_game([str(binary.resolve(strict=True)), '--resources', str(lab.RESOURCES),
                           '--world', 'harness', '--seed', '1', '--radius', '4'],
                          cwd=scratch, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=900)
    if status.returncode:
        raise RuntimeError('Native fixture failed to run; see private native.log')
    actual = observed(native, fields)
    differences = {k: [expected[k], actual[k]] for k in expected if expected[k] != actual[k]}
    (work / 'differences.json').write_text(json.dumps(differences, indent=2) + '\n', encoding='utf-8')
    print(f'{RECORDING}: {"FAIL" if differences else "PASS"}')
    return bool(differences)


if __name__ == '__main__':
    raise SystemExit(main())
