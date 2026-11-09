package dev.horizonlab.parity;

import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.io.BufferedReader;
import java.io.ByteArrayInputStream;
import java.io.DataInputStream;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.io.PrintStream;
import java.io.PrintWriter;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.zip.Deflater;
import java.util.zip.Inflater;
import net.minecraft.SharedConstants;
import net.minecraft.core.UUIDUtil;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.common.ClientboundDisconnectPacket;
import net.minecraft.network.protocol.common.ClientboundKeepAlivePacket;
import net.minecraft.network.protocol.common.ClientboundPingPacket;
import net.minecraft.network.protocol.common.ServerboundClientInformationPacket;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.ServerboundKeepAlivePacket;
import net.minecraft.network.protocol.common.ServerboundPongPacket;
import net.minecraft.network.protocol.common.custom.BrandPayload;
import net.minecraft.network.protocol.configuration.ClientboundFinishConfigurationPacket;
import net.minecraft.network.protocol.configuration.ClientboundSelectKnownPacks;
import net.minecraft.network.protocol.configuration.ConfigurationProtocols;
import net.minecraft.network.protocol.configuration.ServerboundFinishConfigurationPacket;
import net.minecraft.network.protocol.configuration.ServerboundSelectKnownPacks;
import net.minecraft.network.protocol.game.ClientboundChunkBatchFinishedPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;
import net.minecraft.network.protocol.game.ClientboundTrackedWaypointPacket;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.network.protocol.game.ServerboundAcceptTeleportationPacket;
import net.minecraft.network.protocol.game.ServerboundChatCommandPacket;
import net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket;
import net.minecraft.network.protocol.game.ServerboundMovePlayerPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerInputPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerLoadedPacket;
import net.minecraft.network.protocol.handshake.ClientIntent;
import net.minecraft.network.protocol.handshake.ClientIntentionPacket;
import net.minecraft.network.protocol.handshake.HandshakeProtocols;
import net.minecraft.network.protocol.login.ClientboundLoginCompressionPacket;
import net.minecraft.network.protocol.login.ClientboundLoginDisconnectPacket;
import net.minecraft.network.protocol.login.ClientboundLoginFinishedPacket;
import net.minecraft.network.protocol.login.LoginProtocols;
import net.minecraft.network.protocol.login.ServerboundHelloPacket;
import net.minecraft.network.protocol.login.ServerboundLoginAcknowledgedPacket;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.world.entity.PositionMoveRotation;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.phys.Vec3;

/**
 * Players made of vanilla's own protocol code, for the locator bar
 * (tools/test_locator_bar_parity.py). Each joins in offline mode, answers
 * keep-alives, pings, teleports and chunk batches, and records every
 * ClientboundTrackedWaypointPacket it receives, decoded by vanilla's codec
 * (a byte vanilla would not read is an error) and summarised from its bytes.
 *
 * <p>{@code LocatorBarClient HOST PORT OUT} takes its script on stdin, one
 * line at a time: {@code step LABEL} (the label the next packets are recorded
 * under), {@code join NAME}, {@code cmd NAME COMMAND...}, {@code shift NAME
 * on|off} (the shift key, as ServerboundPlayerInputPacket), {@code fly NAME
 * on|off} (ServerboundPlayerAbilitiesPacket), {@code still NAME} (the
 * position reminder LocalPlayer.sendPosition sends every 20 ticks standing
 * still), {@code sync NAME} (runs {@code time query gametime}, whose reply is
 * not recorded, and prints {@code synced NAME} once it is back: the server has
 * handled everything this player sent before, and everything the server sent
 * this player before the reply has been recorded), {@code respawn NAME} (the
 * death screen's button, ServerboundClientCommandPacket PERFORM_RESPAWN),
 * {@code use NAME PITCH} (the main hand's item used looking that way,
 * ServerboundUseItemPacket), {@code stab NAME} (a jab, ServerboundPlayerActionPacket
 * STAB), {@code arc NAME TICK} (a jump straight ahead, one position a tick as
 * LocalPlayer sends them, each in the server's next tick, with a STAB ahead of
 * the position of TICK, or none when negative), {@code sprint NAME} (START_SPRINTING,
 * ServerboundPlayerCommandPacket), {@code pickup NAME SLOT} (a left click on that
 * slot of the player's inventory menu, claiming nothing), {@code quit NAME},
 * {@code
 * players on|off} (whether each player also records the other players added
 * to and removed from its level, and its own respawns into a dimension),
 * {@code chat on|off} (whether the system chat each player receives is
 * recorded too, as component JSON), {@code state on|off} (whether each player
 * also records what the server changes about it: its game mode, teleports,
 * motion, inventory slots, effects and death) and {@code end}, which writes each
 * player's lines to OUT. {@code boats on|off} records boat paddle metadata;
 * {@code hooks on|off} records the entity a fishing bobber hooks, by type;
 * {@code paddle NAME LEFT RIGHT} sends the two boolean paddle states;
 * {@code sounds on|off} records typed sound packets and their complete payloads;
 * {@code events on|off} records each level event: its id, block and data;
 * {@code spawns on|off} records each entity other than a player added to the
 * level, by type and position.
 */
public final class LocatorBarClient {
    private static volatile boolean tickSteps;
    private static volatile String step = "start";
    private static volatile boolean players;
    private static volatile boolean chat;
    private static volatile boolean flatChat;
    private static volatile boolean advancements;
    private static volatile boolean listSyncs;
    private static Path partialTrace;
    private static final Object traceLock = new Object();
    private static volatile boolean boats;
    private static volatile boolean hooks;
    private static volatile boolean state;
    private static volatile boolean sounds;
    private static volatile boolean events;
    private static volatile boolean spawns;
    private static volatile boolean menus;
    private static volatile boolean components;
    private static com.google.gson.JsonArray spans(net.minecraft.network.chat.Component message) {
        com.google.gson.JsonArray out = new com.google.gson.JsonArray();
        message.visit((style, text) -> {
            if (text.isEmpty()) {
                return java.util.Optional.empty();
            }
            com.google.gson.JsonArray key = styleKey(style);
            if (!out.isEmpty()) {
                com.google.gson.JsonArray last = out.get(out.size() - 1).getAsJsonArray();
                if (last.get(1).equals(key)) {
                    last.set(0, new com.google.gson.JsonPrimitive(last.get(0).getAsString() + text));
                    return java.util.Optional.empty();
                }
            }
            com.google.gson.JsonArray span = new com.google.gson.JsonArray();
            span.add(text);
            span.add(key);
            out.add(span);
            return java.util.Optional.empty();
        }, net.minecraft.network.chat.Style.EMPTY);
        return out;
    }

    private static com.google.gson.JsonArray styleKey(net.minecraft.network.chat.Style style) {
        com.google.gson.JsonArray key = new com.google.gson.JsonArray();
        if (style.getColor() == null) {
            key.add(com.google.gson.JsonNull.INSTANCE);
        } else {
            key.add(style.getColor().getValue() & 0xFFFFFF);
        }
        key.add(style.isBold());
        key.add(style.isItalic());
        key.add(style.isUnderlined());
        key.add(style.isStrikethrough());
        key.add(style.isObfuscated());
        return key;
    }

    private static final Map<UUID, String> NAMES = new LinkedHashMap<>();

    public static void main(String[] args) throws Exception {
        // Bootstrap sends System.out to the logger: `synced` lines go to the real stdout.
        PrintStream stdout = new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8);
        ParityData.init();
        String host = args[0];
        int port = Integer.parseInt(args[1]);
        Path output = Path.of(args[2]);
        partialTrace = output.resolveSibling(output.getFileName() + ".partial");
        Map<String, Player> players = new LinkedHashMap<>();
        List<Player> sessions = new ArrayList<>();
        BufferedReader script = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        String line;
        while ((line = script.readLine()) != null) {
            String[] w = line.trim().split(" ", 3);
            try {
                switch (w[0]) {
                    case "step" -> step = w[1];
                    case "join" -> {
                        Player p = new Player(w[1]);
                        players.put(w[1], p);
                        sessions.add(p);
                        NAMES.put(UUIDUtil.createOfflinePlayerUUID(w[1]), w[1]);
                        p.join(host, port);
                    }
                    case "cmd" -> players.get(w[1]).command(w[2]);
                    case "send-cmd" -> players.get(w[1]).send(new ServerboundChatCommandPacket(w[2]));
                    case "shift" -> players.get(w[1]).send(new ServerboundPlayerInputPacket(
                        new Input(false, false, false, false, false, w[2].equals("on"), false)));
                    case "fly" -> {
                        net.minecraft.world.entity.player.Abilities abilities = new net.minecraft.world.entity.player.Abilities();
                        abilities.flying = w[2].equals("on");
                        players.get(w[1]).send(new net.minecraft.network.protocol.game.ServerboundPlayerAbilitiesPacket(abilities));
                    }
                    case "ticksteps" -> tickSteps = w[1].equals("on");
                    case "still" -> players.get(w[1]).still();
                    case "respawn" -> players.get(w[1]).send(new net.minecraft.network.protocol.game.ServerboundClientCommandPacket(
                        net.minecraft.network.protocol.game.ServerboundClientCommandPacket.Action.PERFORM_RESPAWN));
                    case "use-on" -> {
                        String[] a = w[2].split(" ");
                        players.get(w[1]).useOn(new net.minecraft.core.BlockPos(Integer.parseInt(a[0]), Integer.parseInt(a[1]), Integer.parseInt(a[2])));
                    }
                    case "use" -> players.get(w[1]).use(Float.parseFloat(w[2]));
                    case "stab" -> players.get(w[1]).stab();
                    case "arc" -> players.get(w[1]).arc(Integer.parseInt(w[2]));
                    case "sprint" -> players.get(w[1]).sprint();
                    case "pickup" -> players.get(w[1]).send(new net.minecraft.network.protocol.game.ServerboundContainerClickPacket(
                        0, 0, Short.parseShort(w[2]), (byte) 0, net.minecraft.world.inventory.ContainerInput.PICKUP,
                        new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>(), net.minecraft.network.HashedStack.EMPTY));
                    // A stale state requests the server's complete authoritative post-click snapshot.
                    case "click" -> {
                        String[] a = w[2].split(" ");
                        Player p = players.get(w[1]);
                        int window = Integer.parseInt(a[0]);
                        p.send(new net.minecraft.network.protocol.game.ServerboundContainerClickPacket(
                            window, -1, Short.parseShort(a[1]), (byte) 0,
                            net.minecraft.world.inventory.ContainerInput.PICKUP,
                            new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>(), net.minecraft.network.HashedStack.EMPTY));
                    }
                    case "place-recipe" -> {
                        String[] a = w[2].split(" ");
                        players.get(w[1]).send(new net.minecraft.network.protocol.game.ServerboundPlaceRecipePacket(
                            Integer.parseInt(a[0]), new net.minecraft.world.item.crafting.display.RecipeDisplayId(Integer.parseInt(a[1])),
                            Boolean.parseBoolean(a[2])));
                    }
                    case "close-menu" -> players.get(w[1]).send(
                        new net.minecraft.network.protocol.game.ServerboundContainerClosePacket(Integer.parseInt(w[2])));
                    case "menus" -> menus = w[1].equals("on");
                    case "components" -> components = w[1].equals("on");
                    case "dump" -> {
                        writeSessions(output, sessions);
                        stdout.println("dumped " + w[1]);
                    }
                    case "sync" -> {
                        try {
                            players.get(w[1]).sync();
                        } finally {
                            stdout.println("synced " + w[1]);
                        }
                    }
                    case "quit" -> players.get(w[1]).close();
                    case "players" -> LocatorBarClient.players = w[1].equals("on");
                    case "chat" -> chat = w[1].equals("on");
                    case "flat-chat" -> flatChat = w[1].equals("on");
                    case "advancements" -> advancements = w[1].equals("on");
                    case "list-syncs" -> listSyncs = w[1].equals("on");
                    case "adv-snapshot" -> {
                        String[] a = w[2].split(" ", 2);
                        try {
                            players.get(w[1]).advancementSnapshot(a[0], a[1].split(","));
                        } finally {
                            writeSessions(output, sessions);
                            stdout.println("adv-snapshotted " + a[0] + " " + w[1]);
                        }
                    }
                    case "boats" -> boats = w[1].equals("on");
                    case "hooks" -> hooks = w[1].equals("on");
                    case "paddle" -> {
                        String[] sides = w[2].split(" ");
                        players.get(w[1]).send(new net.minecraft.network.protocol.game.ServerboundPaddleBoatPacket(
                            Boolean.parseBoolean(sides[0]), Boolean.parseBoolean(sides[1])));
                    }
                    case "state" -> state = w[1].equals("on");
                    case "sounds" -> sounds = w[1].equals("on");
                    case "events" -> events = w[1].equals("on");
                    case "spawns" -> spawns = w[1].equals("on");
                    case "end" -> {
                        writeSessions(output, sessions);
                        for (Player p : sessions) p.close();
                        System.exit(0);
                    }
                    default -> throw new IllegalArgumentException("unknown script line " + line);
                }
            } catch (Exception e) {
                Player p = w.length > 1 ? players.get(w[1]) : null;
                if (p != null) p.record("error " + w[0] + " " + e);
                else System.err.println("error " + line + ": " + e);
            }
        }
        System.exit(1);
    }

    private static void writeSessions(Path output, List<Player> sessions) throws Exception {
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(output, StandardCharsets.UTF_8))) {
            for (Player p : sessions) {
                synchronized (p.lines) {
                    for (String line : p.lines) out.println(p.name + " " + line);
                }
            }
        }
    }

    private static com.google.gson.JsonElement menuStack(net.minecraft.world.item.ItemStack stack) throws Exception {
        if (stack.isEmpty()) return com.google.gson.JsonNull.INSTANCE;
        var tag = net.minecraft.world.item.ItemStack.CODEC.encodeStart(
            ParityData.access.createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE), stack).getOrThrow();
        var bytes = new java.io.ByteArrayOutputStream();
        net.minecraft.nbt.NbtIo.write((net.minecraft.nbt.CompoundTag) tag, new java.io.DataOutputStream(bytes));
        var json = new com.google.gson.JsonObject();
        json.addProperty("snbt", tag.toString());
        json.addProperty("nbt", java.util.HexFormat.of().formatHex(bytes.toByteArray()));
        return json;
    }

    /** The waypoint packet's fields, read back from its bytes (the operation and type are private enums). */
    static String waypoint(byte[] payload) {
        FriendlyByteBuf b = new FriendlyByteBuf(Unpooled.wrappedBuffer(payload));
        b.readVarInt(); // the packet id
        String operation = switch (b.readVarInt()) {
            case 0 -> "track";
            case 1 -> "untrack";
            case 2 -> "update";
            default -> "operation?";
        };
        String id = b.readBoolean() ? NAMES.getOrDefault(b.readUUID(), "uuid?") : "name:" + b.readUtf();
        String style = b.readUtf();
        String color = b.readBoolean() ? String.format("#%02x%02x%02x", b.readUnsignedByte(), b.readUnsignedByte(), b.readUnsignedByte()) : "-";
        String target = switch (b.readVarInt()) {
            case 0 -> "empty";
            case 1 -> "block " + b.readVarInt() + " " + b.readVarInt() + " " + b.readVarInt();
            case 2 -> "chunk " + b.readVarInt() + " " + b.readVarInt();
            case 3 -> "azimuth " + b.readFloat();
            default -> "type?";
        };
        return operation + " " + id + " " + style + " " + color + " " + target;
    }

    /** One connection: login and configuration in the caller's thread, then play on its own. */
    private static boolean isBoat(net.minecraft.world.entity.EntityType<?> type) {
        String id = net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(type).toString();
        return id.endsWith("_boat") || id.endsWith("_raft");
    }

    private static final class Player {
        final String name;
        final List<String> lines = new ArrayList<>();
        final Map<Integer, Integer> menuStates = new java.util.concurrent.ConcurrentHashMap<>();
        private Socket socket;
        private DataInputStream in;
        private OutputStream out;
        private int threshold = -1;
        private ProtocolInfo<?> inbound;
        private ProtocolInfo<?> outbound;
        private volatile PositionMoveRotation position = new PositionMoveRotation(Vec3.ZERO, Vec3.ZERO, 0, 0);
        private boolean loaded;
        /** This player's own entity id (ClientboundLoginPacket). */
        private volatile int id = -1;
        private int vehicle = -1;
        /** The other players this one was sent, by entity id. */
        private final java.util.Set<Integer> boatIds = new java.util.HashSet<>();
        private final Map<Integer, String> entityTypes = new java.util.HashMap<>();
        private final Map<Integer, String> seen = new java.util.HashMap<>();
        private final Object replyLock = new Object();
        private int replies;
        private int syncs;
        private int listReplies;
        private UUID profileUuid;
        private String profileName;
        private int advancementResets;
        private int advancementReceipts;
        private final net.minecraft.advancements.AdvancementTree advancementTree = new net.minecraft.advancements.AdvancementTree();
        private final Map<net.minecraft.advancements.AdvancementHolder, net.minecraft.advancements.AdvancementProgress> advancementProgress = new LinkedHashMap<>();
        /** The game time in the latest sync's reply. */
        private long gameTime;

        Player(String name) {
            this.name = name;
        }

        /** Read-only replay of the pinned ClientAdvancements.update state path. */
        void advancementUpdate(net.minecraft.network.protocol.game.ClientboundUpdateAdvancementsPacket packet, byte[] payload) {
            synchronized (replyLock) {
                if (packet.shouldReset()) {
                    advancementTree.clear();
                    advancementProgress.clear();
                    advancementResets++;
                }
                advancementTree.remove(packet.removed());
                advancementTree.addAll(packet.added().stream().map(a -> a.advancement()).toList());
                for (var entry : packet.progress().entrySet()) {
                    var node = advancementTree.get(entry.getKey());
                    if (node != null) {
                        entry.getValue().update(node.advancement().requirements());
                        advancementProgress.put(node.holder(), entry.getValue());
                    }
                }
                advancementReceipts++;
                var raw = new com.google.gson.JsonObject();
                raw.addProperty("wire", java.util.HexFormat.of().formatHex(payload));
                raw.addProperty("reset", packet.shouldReset());
                raw.addProperty("receipt", advancementReceipts);
                raw.add("removed", new com.google.gson.Gson().toJsonTree(packet.removed().stream().map(Object::toString).toList()));
                var added = new com.google.gson.JsonObject();
                for (var a : packet.added()) added.add(a.advancement().id().toString(),
                    new com.google.gson.Gson().toJsonTree(a.advancement().value().requirements().requirements()));
                raw.add("added_requirements", added);
                var progress = new com.google.gson.JsonObject();
                for (var entry : packet.progress().entrySet()) progress.add(entry.getKey().toString(),
                    net.minecraft.advancements.AdvancementProgress.CODEC.encodeStart(com.mojang.serialization.JsonOps.INSTANCE, entry.getValue()).getOrThrow());
                raw.add("progress", progress);
                record("adv-wire " + raw);
            }
        }

        void listReceipt() throws Exception {
            synchronized (replyLock) {
                int before = listReplies;
                send(new ServerboundChatCommandPacket("list"));
                long deadline = System.currentTimeMillis() + 10_000;
                while (listReplies == before && System.currentTimeMillis() < deadline) replyLock.wait(100);
                if (listReplies == before) throw new IllegalStateException("no actual /list receipt for " + name);
            }
        }

        void advancementSnapshot(String label, String[] identifiers) throws Exception {
            // /list requires no operator permission. A fixed same-connection
            // receipt follows all mutations without stepping the frozen world.
            listReceipt();
            synchronized (replyLock) {
                if (profileUuid == null || !name.equals(profileName) || advancementResets == 0)
                    throw new IllegalStateException("advancement stream lacks login/reset identity: " + name);
                var out = new com.google.gson.JsonObject();
                out.addProperty("label", label);
                out.addProperty("name", profileName);
                out.addProperty("uuid", profileUuid.toString());
                out.addProperty("entity_id", id);
                out.addProperty("reset_count", advancementResets);
                out.addProperty("packet_count", advancementReceipts);
                out.addProperty("receipt_count", listReplies);
                var states = new com.google.gson.JsonObject();
                for (String identifier : identifiers) {
                    var node = advancementTree.get(net.minecraft.resources.Identifier.parse(identifier));
                    var state = new com.google.gson.JsonObject();
                    state.addProperty("present", node != null);
                    if (node != null) {
                        var progress = advancementProgress.get(node.holder());
                        state.addProperty("progress_received", progress != null);
                        if (progress != null) {
                            state.addProperty("done", progress.isDone());
                            var criteria = new java.util.ArrayList<String>();
                            progress.getCompletedCriteria().forEach(criteria::add);
                            java.util.Collections.sort(criteria);
                            state.add("criteria", new com.google.gson.Gson().toJsonTree(criteria));
                        }
                    }
                    states.add(identifier, state);
                }
                out.add("progress", states);
                record("adv-snapshot " + out);
            }
        }

        /** Runs a command and waits for its feedback, so a slow server still takes each step in its step. */
        void command(String command) throws Exception {
            synchronized (replyLock) {
                int before = replies;
                int beforeSync = syncs;
                send(new ServerboundChatCommandPacket(command));
                long until = System.nanoTime() + 30_000_000_000L;
                while (replies == before && syncs == beforeSync) {
                    long left = (until - System.nanoTime()) / 1_000_000L;
                    if (left <= 0) {
                        record("error no reply to /" + command);
                        return;
                    }
                    replyLock.wait(left);
                }
            }
        }

        /** A round trip through the server thread, its reply left out of the transcript. */
        void sync() throws Exception {
            if (listSyncs) {
                listReceipt();
                return;
            }
            synchronized (replyLock) {
                int before = syncs;
                send(new ServerboundChatCommandPacket("time query gametime"));
                long until = System.nanoTime() + 30_000_000_000L;
                while (syncs == before) {
                    long left = (until - System.nanoTime()) / 1_000_000L;
                    if (left <= 0) {
                        record("error no reply to sync");
                        return;
                    }
                    replyLock.wait(left);
                }
            }
        }

        /**
         * Waits until the game time moves on from the latest sync's: the server has run the tick that handles what
         * this player sent since. A sleep of 50 ms drifts from the server's ticks, and a jump then gets a tick of
         * gravity more or less. Vanilla runs commands between ticks and movement in the tick (its PacketProcessor),
         * Horizon both at the tick's start: the latest reply, not a new sync, is the tick to wait past on both.
         * Returns how many ticks the server ran: more than one when it caught up on a busy machine.
         */
        long nextTick() throws Exception {
            long now = gameTime;
            long until = System.nanoTime() + 30_000_000_000L;
            while (gameTime == now) {
                if (System.nanoTime() > until) {
                    record("error no tick after " + now);
                    return 0;
                }
                Thread.sleep(1);
                sync();
            }
            return gameTime - now;
        }

        void record(String line) {
            synchronized (lines) {
                String entry = "step=" + step + " " + line;
                lines.add(entry);
                if ((advancements || flatChat) && partialTrace != null) {
                    synchronized (traceLock) {
                        try {
                            Files.writeString(partialTrace, name + " " + entry + "\n",
                                StandardCharsets.UTF_8, java.nio.file.StandardOpenOption.CREATE, java.nio.file.StandardOpenOption.APPEND);
                        } catch (java.io.IOException error) {
                            throw new java.io.UncheckedIOException(error);
                        }
                    }
                }
            }
        }

        private int sequence;

        void use(float pitch) throws Exception {
            send(new net.minecraft.network.protocol.game.ServerboundUseItemPacket(
                net.minecraft.world.InteractionHand.MAIN_HAND, ++sequence, position.yRot(), pitch));
        }

        /** Use the named block's top face independently of the player's last look direction. */
        void useOn(net.minecraft.core.BlockPos pos) throws Exception {
            send(new net.minecraft.network.protocol.game.ServerboundUseItemOnPacket(
                net.minecraft.world.InteractionHand.MAIN_HAND,
                new net.minecraft.world.phys.BlockHitResult(Vec3.atBottomCenterOf(pos).add(0, 1, 0),
                    net.minecraft.core.Direction.UP, pos, false), ++sequence));
        }

        void stab() throws Exception {
            send(new net.minecraft.network.protocol.game.ServerboundPlayerActionPacket(
                net.minecraft.network.protocol.game.ServerboundPlayerActionPacket.Action.STAB,
                net.minecraft.core.BlockPos.ZERO, net.minecraft.core.Direction.DOWN));
        }

        /** The packet's constructor wants an entity, which this player doesn't have: read from its own bytes. */
        void sprint() throws Exception {
            FriendlyByteBuf bytes = new FriendlyByteBuf(Unpooled.buffer());
            bytes.writeVarInt(id);
            bytes.writeEnum(net.minecraft.network.protocol.game.ServerboundPlayerCommandPacket.Action.START_SPRINTING);
            bytes.writeVarInt(0);
            send(net.minecraft.network.protocol.game.ServerboundPlayerCommandPacket.STREAM_CODEC.decode(bytes));
        }

        /** LivingEntity.jumpFromGround's 0.42, then gravity 0.08 and drag 0.98 a tick; the clicks come first in a tick. */
        void arc(int stabTick) throws Exception {
            PositionMoveRotation at = position;
            Vec3 start = at.position();
            double y = start.y;
            double z = start.z;
            double up = 0.42;
            // A tick of its own, as in still: the join's position has none behind it.
            send(net.minecraft.network.protocol.game.ServerboundClientTickEndPacket.INSTANCE);
            sync();
            for (int tick = 0; tick < 13; tick++) {
                if (tick == stabTick) stab();
                y = Math.max(start.y, y + up);
                up = (up - 0.08) * 0.98;
                z += 0.3;
                boolean landed = tick > 0 && y == start.y;
                send(new ServerboundMovePlayerPacket.PosRot(new Vec3(start.x, y, z), at.yRot(), at.xRot(), landed, false));
                send(net.minecraft.network.protocol.game.ServerboundClientTickEndPacket.INSTANCE);
                long ran = nextTick();
                // A server catching up runs a tick with no move in it: up to the jab, its copy of the player then
                // falls a tick of gravity more, and the motion the jab sends says nothing about the jump (#5656). The
                // jab's own move isn't counted: a server may take what follows the jab a tick later, and the motion
                // is already decided. A catch-up just before the jab still shows in that motion, which is compared.
                if (ran > 1 && tick < stabTick) {
                    record("error the server ran " + ran + " ticks for move " + tick + " of the jump");
                }
                if (landed) break;
            }
            position = new PositionMoveRotation(new Vec3(start.x, y, z), Vec3.ZERO, at.yRot(), at.xRot());
        }

        void still() throws Exception {
            PositionMoveRotation at = position;
            // A tick of its own: the server takes one position per client tick (receivedPositionThisTick).
            send(net.minecraft.network.protocol.game.ServerboundClientTickEndPacket.INSTANCE);
            send(new ServerboundMovePlayerPacket.PosRot(at.position(), at.yRot(), at.xRot(), false, false));
            send(net.minecraft.network.protocol.game.ServerboundClientTickEndPacket.INSTANCE);
        }

        void close() throws Exception {
            if (socket != null) socket.close();
        }

        void join(String host, int port) throws Exception {
            socket = new Socket();
            socket.connect(new InetSocketAddress(host, port), 10_000);
            socket.setTcpNoDelay(true);
            in = new DataInputStream(socket.getInputStream());
            out = socket.getOutputStream();
            outbound = HandshakeProtocols.SERVERBOUND;
            send(new ClientIntentionPacket(SharedConstants.getProtocolVersion(), host, port, ClientIntent.LOGIN));
            outbound = LoginProtocols.SERVERBOUND;
            inbound = LoginProtocols.CLIENTBOUND;
            send(new ServerboundHelloPacket(name, UUIDUtil.createOfflinePlayerUUID(name)));
            while (true) {
                Packet<?> p = read().packet;
                if (p instanceof ClientboundLoginCompressionPacket c) {
                    threshold = c.getCompressionThreshold();
                } else if (p instanceof ClientboundLoginFinishedPacket finished) {
                    profileUuid = finished.gameProfile().id();
                    profileName = finished.gameProfile().name();
                    if (advancements) record("adv-login " + profileName + " " + profileUuid);
                    send(ServerboundLoginAcknowledgedPacket.INSTANCE);
                    break;
                } else if (p instanceof ClientboundLoginDisconnectPacket d) {
                    throw new IllegalStateException("login refused: " + d.reason().getString());
                }
            }
            outbound = ConfigurationProtocols.SERVERBOUND;
            inbound = ConfigurationProtocols.CLIENTBOUND;
            send(new ServerboundCustomPayloadPacket(new BrandPayload("vanilla")));
            send(new ServerboundClientInformationPacket(ClientInformation.createDefault()));
            while (true) {
                Packet<?> p = read().packet;
                if (p instanceof ClientboundSelectKnownPacks k) {
                    send(new ServerboundSelectKnownPacks(k.knownPacks()));
                } else if (p instanceof ClientboundKeepAlivePacket k) {
                    send(new ServerboundKeepAlivePacket(k.getId()));
                } else if (p instanceof ClientboundPingPacket k) {
                    send(new ServerboundPongPacket(k.getId()));
                } else if (p instanceof ClientboundFinishConfigurationPacket) {
                    send(ServerboundFinishConfigurationPacket.INSTANCE);
                    break;
                } else if (p instanceof ClientboundDisconnectPacket d) {
                    throw new IllegalStateException("disconnected: " + d.reason().getString());
                }
            }
            GameProtocols.Context context = new GameProtocols.Context() {
                @Override
                public boolean hasInfiniteMaterials() {
                    return true;
                }

                @Override
                public boolean canUseCommandBlocks() {
                    return true;
                }
            };
            outbound = GameProtocols.SERVERBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(ParityData.access), context);
            inbound = GameProtocols.CLIENTBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(ParityData.access));
            Thread reader = new Thread(this::play, name);
            reader.setDaemon(true);
            reader.start();
        }

        private void recordMenu(Packet<?> p, byte[] payload) throws Exception {
            var j = new com.google.gson.JsonObject();
            j.addProperty("packet", p.type().id().toString());
            j.addProperty("wire", java.util.HexFormat.of().formatHex(payload));
            if (p instanceof net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket c) {
                j.addProperty("kind", "content");
                j.addProperty("window", c.containerId());
                j.addProperty("state", c.stateId());
                menuStates.put(c.containerId(), c.stateId());
                var items = new com.google.gson.JsonArray();
                for (var stack : c.items()) items.add(menuStack(stack));
                j.add("items", items);
                j.add("carried", menuStack(c.carriedItem()));
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket c) {
                j.addProperty("kind", "slot");
                j.addProperty("window", c.getContainerId());
                j.addProperty("state", c.getStateId());
                menuStates.put(c.getContainerId(), c.getStateId());
                j.addProperty("slot", c.getSlot());
                j.add("item", menuStack(c.getItem()));
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetCursorItemPacket c) {
                j.addProperty("kind", "cursor");
                j.add("item", menuStack(c.contents()));
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetPlayerInventoryPacket c) {
                j.addProperty("kind", "inventory");
                j.addProperty("slot", c.slot());
                j.add("item", menuStack(c.contents()));
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundOpenScreenPacket c) {
                j.addProperty("kind", "open");
                j.addProperty("window", c.getContainerId());
                j.addProperty("type", net.minecraft.core.registries.BuiltInRegistries.MENU.getKey(c.getType()).toString());
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundRecipeBookAddPacket c) {
                j.addProperty("kind", "recipes");
                j.addProperty("replace", c.replace());
                var entries = new com.google.gson.JsonArray();
                for (var entry : c.entries()) {
                    var value = new com.google.gson.JsonObject();
                    value.addProperty("id", entry.contents().id().index());
                    value.addProperty("flags", entry.flags());
                    value.add("display", net.minecraft.world.item.crafting.display.RecipeDisplay.CODEC.encodeStart(
                        ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), entry.contents().display()).getOrThrow());
                    var contents = entry.contents();
                    if (contents.group().isPresent()) value.addProperty("group", contents.group().getAsInt());
                    value.addProperty("category", net.minecraft.core.registries.BuiltInRegistries.RECIPE_BOOK_CATEGORY
                        .getKey(contents.category()).toString());
                    contents.craftingRequirements().ifPresent(list -> value.add("requirements",
                        net.minecraft.world.item.crafting.Ingredient.CODEC.listOf().encodeStart(
                            ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), list).getOrThrow()));
                    entries.add(value);
                }
                j.add("entries", entries);
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundRecipeBookRemovePacket c) {
                j.addProperty("kind", "recipes-removed");
                var ids = new com.google.gson.JsonArray();
                for (var id : c.recipes()) ids.add(id.index());
                j.add("ids", ids);
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundPlaceGhostRecipePacket c) {
                j.addProperty("kind", "ghost");
                j.addProperty("window", c.containerId());
                j.add("display", net.minecraft.world.item.crafting.display.RecipeDisplay.CODEC.encodeStart(
                    ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), c.recipeDisplay()).getOrThrow());
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundLoginPacket c) {
                j.addProperty("kind", "login");
                j.addProperty("mode", c.commonPlayerSpawnInfo().gameType().getId());
            } else if (p instanceof ClientboundSystemChatPacket c) {
                j.addProperty("kind", "chat");
                j.add("text", net.minecraft.network.chat.ComponentSerialization.CODEC.encodeStart(
                    ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), c.content()).getOrThrow());
                j.addProperty("overlay", c.overlay());
            } else return;
            record("menu-wire " + j);
        }

        private void play() {
            try {
                while (true) {
                    Received r = read();
                    Packet<?> p = r.packet;
                    if (p instanceof net.minecraft.network.protocol.game.ClientboundLoginPacket l) {
                        id = l.playerId();
                    }
                    if (tickSteps && p instanceof net.minecraft.network.protocol.game.ClientboundTickingStepPacket t)
                        record("tick-step " + t.tickSteps());
                    if (tickSteps && p instanceof net.minecraft.network.protocol.game.ClientboundBlockEventPacket e
                        && e.getPos().equals(new net.minecraft.core.BlockPos(0, 80, 0)))
                        record("piston-event " + e.getB0() + " " + e.getB1());
                    if (advancements && p instanceof net.minecraft.network.protocol.game.ClientboundUpdateAdvancementsPacket a) {
                        advancementUpdate(a, r.payload);
                    }

                    if (sounds && p instanceof net.minecraft.network.protocol.game.ClientboundSoundPacket sound) {
                        com.google.gson.JsonObject j = new com.google.gson.JsonObject();
                        j.addProperty("event", sound.getSound().value().location().toString());
                        j.addProperty("source", sound.getSource().ordinal());
                        j.addProperty("source_name", sound.getSource().name());
                        com.google.gson.JsonArray at = new com.google.gson.JsonArray();
                        at.add(sound.getX()); at.add(sound.getY()); at.add(sound.getZ());
                        j.add("position", at);
                        j.addProperty("volume", sound.getVolume());
                        j.addProperty("pitch", sound.getPitch());
                        j.addProperty("volume_bits", Float.floatToRawIntBits(sound.getVolume()));
                        j.addProperty("pitch_bits", Float.floatToRawIntBits(sound.getPitch()));
                        j.addProperty("seed", sound.getSeed());
                        j.addProperty("payload_hex", java.util.HexFormat.of().formatHex(r.payload));
                        record("sound-wire " + j);
                    }
                    if (state) recordState(p);
                    if (menus) recordMenu(p, r.payload);
                    if (components) {
                        var evidence = ComponentWireCapture.inspect(p, r.payload);
                        if (evidence != null) record("component-wire " + evidence);
                    }
                    if (hooks && p instanceof net.minecraft.network.protocol.game.ClientboundAddEntityPacket a) {
                        entityTypes.put(a.getId(), net.minecraft.world.entity.EntityType.getKey(a.getType()).toString());
                    }
                    // FishingHook.DATA_HOOKED_ENTITY (id 8) holds the hooked entity's id + 1, or 0.
                    if (hooks && p instanceof net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket data
                        && "minecraft:fishing_bobber".equals(entityTypes.get(data.id()))) {
                        for (var value : data.packedItems()) {
                            if (value.id() == 8 && value.value() instanceof Integer hooked) {
                                record("hooked " + (hooked > 0 ? entityTypes.get(hooked - 1) : "none"));
                            }
                        }
                    }
                    if (boats && p instanceof net.minecraft.network.protocol.game.ClientboundAddEntityPacket a
                        && isBoat(a.getType())) {
                        boatIds.add(a.getId());
                    }
                    if (boats && p instanceof net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket data
                        && boatIds.contains(data.id())) {
                        for (var value : data.packedItems()) {
                            if ((value.id() == 11 || value.id() == 12) && value.value() instanceof Boolean) {
                                record("paddle " + value.id() + " " + value.value());
                            }
                        }
                    }
                    if (spawns && p instanceof net.minecraft.network.protocol.game.ClientboundAddEntityPacket a
                        && a.getType() != net.minecraft.world.entity.EntityTypes.PLAYER) {
                        record(String.format(java.util.Locale.ROOT, "spawn %s %.4f %.4f %.4f",
                            net.minecraft.world.entity.EntityType.getKey(a.getType()), a.getX(), a.getY(), a.getZ()));
                    }
                    if (p instanceof net.minecraft.network.protocol.game.ClientboundAddEntityPacket a
                        && a.getType() == net.minecraft.world.entity.EntityTypes.PLAYER) {
                        String who = NAMES.getOrDefault(a.getUUID(), "uuid?");
                        seen.put(a.getId(), who);
                        if (players) record("add " + who + " at " + a.getX() + "," + a.getY() + "," + a.getZ());
                    } else if (p instanceof net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket rm) {
                        rm.entityIds().forEach(id -> {
                            String who = seen.remove(id);
                            if (who != null && players) record("remove " + who);
                        });
                    } else if (p instanceof net.minecraft.network.protocol.game.ClientboundRespawnPacket rs) {
                        if (players) record("respawn " + rs.commonPlayerSpawnInfo().dimension().identifier());
                        seen.clear();
                    } else if (p instanceof ClientboundTrackedWaypointPacket) {
                        record(waypoint(r.payload));
                    } else if (p instanceof ClientboundSystemChatPacket c
                        && c.content().getContents() instanceof net.minecraft.network.chat.contents.TranslatableContents t
                        && t.getKey().equals("commands.time.query.gametime")) {
                        Object time = t.getArgs()[0];
                        synchronized (replyLock) {
                            gameTime = Long.parseLong(time instanceof net.minecraft.network.chat.Component text
                                ? text.getString() : String.valueOf(time));
                            syncs++;
                            replyLock.notifyAll();
                        }
                    } else if (advancements && p instanceof ClientboundSystemChatPacket c
                        && c.content().getContents() instanceof net.minecraft.network.chat.contents.TranslatableContents t
                        && t.getKey().equals("commands.list.players")) {
                        synchronized (replyLock) {
                            record("adv-receipt " + net.minecraft.network.chat.ComponentSerialization.CODEC.encodeStart(
                                ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), c.content()).getOrThrow());
                            listReplies++;
                            replyLock.notifyAll();
                        }
                    } else if (p instanceof ClientboundSystemChatPacket c) {
                        if (flatChat) record("chat-flat " + spans(c.content()));
                        if (chat) {
                            record("chat " + (c.overlay() ? "overlay " : "") + net.minecraft.network.chat.ComponentSerialization.CODEC
                                .encodeStart(ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), c.content())
                                .getOrThrow());
                        }
                        synchronized (replyLock) {
                            replies++;
                            replyLock.notifyAll();
                        }
                    } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetEntityMotionPacket m && m.id() == id) {
                        if (state) {
                            Vec3 v = m.movement();
                            record(String.format(java.util.Locale.ROOT, "motion %.4f %.4f %.4f", v.x, v.y, v.z));
                        }
                    } else if (p instanceof net.minecraft.network.protocol.game.ClientboundLevelEventPacket e) {
                        if (events) {
                            net.minecraft.core.BlockPos at = e.getPos();
                            record("event " + e.getType() + " " + at.getX() + " " + at.getY() + " " + at.getZ()
                                + " " + e.getData());
                        }
                    } else if (p instanceof ClientboundKeepAlivePacket k) {
                        send(new ServerboundKeepAlivePacket(k.getId()));
                    } else if (p instanceof ClientboundPingPacket k) {
                        send(new ServerboundPongPacket(k.getId()));
                    } else if (p instanceof ClientboundPlayerPositionPacket t) {
                        position = PositionMoveRotation.calculateAbsolute(position, t.change(), t.relatives());
                        Vec3 at = position.position();
                        if (state) {
                            record(String.format(java.util.Locale.ROOT, "teleport %.3f %.3f %.3f %.1f %.1f",
                                at.x, at.y, at.z, position.yRot(), position.xRot()));
                        }
                        send(new ServerboundAcceptTeleportationPacket(t.id(), at.x, at.y, at.z, position.yRot(), position.xRot()));
                        // Only the spawn teleport is answered with a position too (VanillaProtocolClient).
                        if (!loaded) {
                            send(new ServerboundMovePlayerPacket.PosRot(at, position.yRot(), position.xRot(), false, false));
                            send(new ServerboundPlayerLoadedPacket());
                            loaded = true;
                        }
                    } else if (p instanceof ClientboundChunkBatchFinishedPacket) {
                        send(new ServerboundChunkBatchReceivedPacket(64.0F));
                    } else if (p instanceof ClientboundDisconnectPacket d) {
                        record("disconnected " + d.reason().getString());
                        return;
                    }
                }
            } catch (Exception e) {
                if (!socket.isClosed()) record("error play " + e);
            }
        }

        /** What the server changed about this player ({@code state on}). */
        private void recordState(Packet<?> p) {
            if (p instanceof net.minecraft.network.protocol.game.ClientboundGameEventPacket g
                && g.getEvent() == net.minecraft.network.protocol.game.ClientboundGameEventPacket.CHANGE_GAME_MODE) {
                record("gamemode " + (int) g.getParam());
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetPassengersPacket passengers) {
                boolean aboard = java.util.Arrays.stream(passengers.getPassengers()).anyMatch(passenger -> passenger == id);
                if (aboard || passengers.getVehicle() == vehicle) {
                    vehicle = aboard ? passengers.getVehicle() : -1;
                    record("passenger " + aboard);
                }
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket c) {
                record("slot " + c.getContainerId() + " " + c.getSlot() + " " + item(c.getItem()));
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetPlayerInventoryPacket i) {
                record("inventory " + i.slot() + " " + item(i.contents()));
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundUpdateMobEffectPacket e && e.getEntityId() == id) {
                record("effect " + e.getEffect().getRegisteredName() + " " + e.getEffectAmplifier() + " " + e.getEffectDurationTicks());
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundRemoveMobEffectPacket e && e.entityId() == id) {
                record("effect-removed " + e.effect().getRegisteredName());
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundPlayerCombatKillPacket k) {
                record("killed " + net.minecraft.network.chat.ComponentSerialization.CODEC
                    .encodeStart(ParityData.access.createSerializationContext(com.mojang.serialization.JsonOps.INSTANCE), k.message())
                    .getOrThrow());
            }
        }

        private static String item(net.minecraft.world.item.ItemStack stack) {
            return stack.isEmpty() ? "empty"
                : net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(stack.getItem()) + " x" + stack.getCount();
        }

        private record Received(Packet<?> packet, byte[] payload) {}

        private Received read() throws Exception {
            byte[] payload = readFrame();
            ByteBuf buf = Unpooled.wrappedBuffer(payload);
            Packet<?> packet = inbound.codec().decode(buf);
            if (buf.readableBytes() > 0) {
                throw new IllegalStateException(packet.type().id() + " was larger than expected: " + buf.readableBytes() + " bytes left");
            }
            return new Received(packet, payload);
        }

        synchronized void send(Packet<?> packet) throws Exception {
            ByteBuf body = Unpooled.buffer();
            ((ProtocolInfo) outbound).codec().encode(body, packet);
            byte[] raw = new byte[body.readableBytes()];
            body.readBytes(raw);
            ByteBuf frame = Unpooled.buffer();
            if (threshold >= 0) {
                ByteBuf inner = Unpooled.buffer();
                if (raw.length >= threshold) {
                    writeVarInt(inner, raw.length);
                    Deflater d = new Deflater();
                    d.setInput(raw);
                    d.finish();
                    byte[] tmp = new byte[raw.length + 64];
                    while (!d.finished()) inner.writeBytes(tmp, 0, d.deflate(tmp));
                    d.end();
                } else {
                    writeVarInt(inner, 0);
                    inner.writeBytes(raw);
                }
                writeVarInt(frame, inner.readableBytes());
                frame.writeBytes(inner);
            } else {
                writeVarInt(frame, raw.length);
                frame.writeBytes(raw);
            }
            byte[] bytes = new byte[frame.readableBytes()];
            frame.readBytes(bytes);
            out.write(bytes);
            out.flush();
        }

        private byte[] readFrame() throws Exception {
            byte[] frame = new byte[readVarInt(in)];
            in.readFully(frame);
            if (threshold < 0) return frame;
            ByteArrayInputStream header = new ByteArrayInputStream(frame);
            int dataLength = readVarInt(header);
            int skip = frame.length - header.available();
            if (dataLength == 0) return java.util.Arrays.copyOfRange(frame, skip, frame.length);
            Inflater inflater = new Inflater();
            inflater.setInput(frame, skip, frame.length - skip);
            byte[] raw = new byte[dataLength];
            int n = inflater.inflate(raw);
            inflater.end();
            if (n != dataLength) throw new IllegalStateException("decompressed " + n + " of " + dataLength + " bytes");
            return raw;
        }
    }

    private static int readVarInt(InputStream in) throws Exception {
        int value = 0;
        for (int shift = 0; shift < 35; shift += 7) {
            int b = in.read();
            if (b < 0) throw new java.io.EOFException();
            value |= (b & 0x7F) << shift;
            if ((b & 0x80) == 0) return value;
        }
        throw new IllegalStateException("VarInt too big");
    }

    private static void writeVarInt(ByteBuf buf, int value) {
        while ((value & ~0x7F) != 0) {
            buf.writeByte((value & 0x7F) | 0x80);
            value >>>= 7;
        }
        buf.writeByte(value);
    }
}
