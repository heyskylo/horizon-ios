"""The serverbound play stream of a capture (tools/net_capture.py), cut into client ticks and decoded field by
field, so two clients' streams can be compared tick by tick (tools/test_wire_stream_parity.py).

A client tick's packets end with ServerboundClientTickEndPacket (Minecraft.tick sends it last). A scenario
starts at the command /wire-mark, which both drivers send at the start of the scenario's first tick.

Some packets are not the tick's: the server's own timing decides them (keep-alive answers, pongs, chunk batch
acknowledgements, teleport acceptances and the move that follows one, chat acknowledgements). They are kept
apart (`frame`), never compared tick by tick, and `misplaced` checks vanilla's rule for them instead: the
client processes the server's packets at the start of a frame (Minecraft.runTick: processQueuedPackets before
the ticks), so none of them lands between two packets of one tick. Keep-alive answers are the exception: the
network thread sends them (ClientCommonPacketListenerImpl.handleKeepAlive).
"""
import struct

# Sent from the frame (the server's packets), not from a tick.
FRAME = {'keep_alive', 'pong', 'chunk_batch_received', 'accept_teleportation', 'chat_ack'}
# Answers to the server that also come from the frame; `misplaced` checks them.
ASYNC = FRAME - {'keep_alive'}


class Reader:
    def __init__(self, data):
        self.data, self.pos = data, 0

    def take(self, n):
        out = self.data[self.pos:self.pos + n]
        if len(out) < n:
            raise ValueError('short packet')
        self.pos += n
        return out

    def varint(self):
        value = shift = 0
        while True:
            b = self.take(1)[0]
            value |= (b & 0x7F) << shift
            if not b & 0x80:
                return value - (1 << 32) if value >= 1 << 31 else value
            shift += 7

    def double(self):
        return struct.unpack('>d', self.take(8))[0]

    def float(self):
        return struct.unpack('>f', self.take(4))[0]

    def ubyte(self):
        return self.take(1)[0]

    def short(self):
        return struct.unpack('>h', self.take(2))[0]

    def bool(self):
        return self.take(1)[0] != 0

    def pos_long(self):
        # BlockPos.asLong: x in the top 26 bits, z in the next 26, y in the low 12, each signed.
        v = struct.unpack('>q', self.take(8))[0]
        signed = lambda value, bits: value - (1 << bits) if value >= 1 << (bits - 1) else value  # noqa: E731
        return [v >> 38, signed(v & 0xFFF, 12), signed((v >> 12) & 0x3FFFFFF, 26)]

    def string(self):
        return self.take(self.varint()).decode('utf-8')

    def rest(self):
        out = self.data[self.pos:]
        self.pos = len(self.data)
        return out.hex()


def flags(v):
    return {'on_ground': bool(v & 1), 'horizontal_collision': bool(v & 2)}


def decode(name, data):
    """The fields of one serverbound play packet (vanilla's codecs); `raw` holds what isn't decoded."""
    r = Reader(data)
    if name == 'move_player_pos':
        return {'x': r.double(), 'y': r.double(), 'z': r.double(), **flags(r.ubyte())}
    if name == 'move_player_pos_rot':
        return {'x': r.double(), 'y': r.double(), 'z': r.double(), 'yaw': r.float(), 'pitch': r.float(),
                **flags(r.ubyte())}
    if name == 'move_player_rot':
        return {'yaw': r.float(), 'pitch': r.float(), **flags(r.ubyte())}
    if name == 'move_player_status_only':
        return flags(r.ubyte())
    if name == 'player_input':
        v = r.ubyte()
        keys = ['forward', 'backward', 'left', 'right', 'jump', 'shift', 'sprint']
        return {'keys': [k for i, k in enumerate(keys) if v >> i & 1]}
    if name == 'player_command':
        actions = ['stop_sleeping', 'start_sprinting', 'stop_sprinting', 'start_riding_jump', 'stop_riding_jump',
                   'open_inventory', 'start_fall_flying']
        entity, action, data_ = r.varint(), r.varint(), r.varint()
        return {'entity': entity, 'action': actions[action] if 0 <= action < len(actions) else action,
                'data': data_}
    if name == 'set_carried_item':
        return {'slot': r.short()}
    if name == 'attack':
        return {'entity': r.varint()}
    if name == 'interact':
        # The point (Vec3.LP_STREAM_CODEC, packed) and the sneak flag stay raw.
        return {'entity': r.varint(), 'hand': r.varint(), 'raw': r.rest()}
    if name == 'use_item':
        return {'hand': r.varint(), 'sequence': r.varint(), 'yaw': r.float(), 'pitch': r.float()}
    if name == 'use_item_on':
        return {'hand': r.varint(), 'pos': r.pos_long(), 'face': r.varint(),
                'cursor': [r.float(), r.float(), r.float()], 'inside': r.bool(), 'world_border': r.bool(),
                'sequence': r.varint()}
    if name == 'player_action':
        return {'action': r.varint(), 'pos': r.pos_long(), 'direction': r.ubyte(), 'sequence': r.varint()}
    if name == 'container_click':
        return {'container': r.varint(), 'state': r.varint(), 'slot': r.short(), 'button': r.ubyte(),
                'input': r.varint(), 'raw': r.rest()}
    if name == 'container_close':
        return {'container': r.varint()}
    if name == 'chat_command':
        return {'command': r.string()}
    return {'raw': r.rest()} if data else {}


def self_id(entries):
    """The player's entity id: the first field (an Int) of the clientbound Login."""
    for e in entries:
        if e.get('dir') == 's2c' and e.get('state') == 'play' and e.get('name') == 'login':
            return struct.unpack('>i', bytes.fromhex(e['data'])[:4])[0]
    return None


def scenarios(entries):
    """One list of ticks for each /wire-mark in the play stream, in order (the ticks after a mark run to the
    next mark); a tick is {'packets': [(name, fields)], 'frame': [(name, fields)], 'misplaced': [name]}.

    A click's state id becomes `stale`: how far it is behind the last one the server sent for that container
    (0 = up to date). The ids themselves count every change the server made since the login, so two runs
    whose earlier scenarios differed number the same click differently."""
    me = self_id(entries)
    play = [e for e in entries if e.get('state') == 'play' and 'name' in e and e.get('dir') in ('c2s', 's2c')]
    runs, current, server_state = [], None, {}
    for e in play:
        name = e['name']
        if e['dir'] == 's2c':
            if name in ('container_set_content', 'container_set_slot'):
                r = Reader(bytes.fromhex(e.get('data', '')))
                container = r.varint()
                server_state[container] = r.varint()
            continue
        fields = decode(name, bytes.fromhex(e.get('data', '')))
        if name == 'container_click':
            last = server_state.get(fields['container'], fields['state'])
            fields = {'container': fields['container'], 'stale': last - fields.pop('state'), **fields}
        if name == 'chat_command' and fields.get('command') == 'wire-mark':
            current = [new_tick()]
            runs.append(current)
            continue
        if current is None:
            continue
        tick = current[-1]
        if 'entity' in fields and fields['entity'] == me:
            fields['entity'] = 'self'
        if name in FRAME:
            tick['frame'].append((name, fields))
            if name in ASYNC and tick['packets']:
                tick['misplaced'].append(name)
            continue
        tick['packets'].append((name, fields))
        if name == 'client_tick_end':
            current.append(new_tick())
    for run in runs:
        name_entities(run)
    return runs


def first_frame(run, name):
    """The first tick whose frame sent `name` (the server's answer it carries), or None."""
    return next((i for i, tick in enumerate(run) if any(n == name for n, _ in tick['frame'])), None)


def new_tick():
    return {'packets': [], 'frame': [], 'misplaced': []}


def name_entities(run):
    """Other entities' ids differ from one run to the next: name them by first appearance (e1, e2...)."""
    names = {}
    for tick in run:
        for _, fields in tick['packets']:
            entity = fields.get('entity')
            if isinstance(entity, int):
                fields['entity'] = names.setdefault(entity, f'e{len(names) + 1}')


def compare(vanilla, horizon, ticks):
    """The ticks 0..ticks where the two streams differ: [(tick, vanilla packets, Horizon packets)]. Fields
    compare exactly, floats included: they are what vanilla's client sends."""
    out = []
    for i in range(ticks + 1):
        v = vanilla[i]['packets'] if i < len(vanilla) else None
        h = horizon[i]['packets'] if i < len(horizon) else None
        if v != h:
            out.append((i, v, h))
    return out


def compare_answered(vanilla, horizon, ticks, av, ah):
    """compare, for a scenario the server answers once (a teleport): the tick each client received the answer
    in (`av`, `ah`) compares with the other client's, every other tick with the same tick, on the script's
    clock. A loaded server answers a few ticks later in one run than in the other, while the script's own
    packets (a button released, the position resent every 20 ticks) keep its clock. Numbers are vanilla's."""
    out = [d for d in compare(vanilla, horizon, ticks) if d[0] not in (av, ah)]
    v = vanilla[av]['packets'] if av < len(vanilla) else None
    h = horizon[ah]['packets'] if ah < len(horizon) else None
    if v != h:
        out.append((av, v, h))
    return sorted(out, key=lambda d: d[0])


def describe(packets):
    if packets is None:
        return '    (no such tick)'
    if not packets:
        return '    (nothing)'
    lines = []
    for name, fields in packets:
        text = ', '.join(f'{k}={round(v, 6) if isinstance(v, float) else v}' for k, v in fields.items())
        lines.append(f'    {name}' + (f' {{{text}}}' if text else ''))
    return '\n'.join(lines)


def field_diff(v, h, names=('vanilla', 'Horizon')):
    """One line per differing field of packets paired by position, with the exact values."""
    a, b = names
    lines = []
    for i, ((vn, vf), (hn, hf)) in enumerate(zip(v or [], h or [])):
        if vn != hn:
            lines.append(f'    #{i}: {a} {vn}, {b} {hn}')
            continue
        for k in sorted(set(vf) | set(hf)):
            if vf.get(k) != hf.get(k):
                lines.append(f'    #{i} {vn}.{k}: {a} {vf.get(k)!r}, {b} {hf.get(k)!r}')
    return lines
