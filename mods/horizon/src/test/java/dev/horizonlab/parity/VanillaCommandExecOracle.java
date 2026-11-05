package dev.horizonlab.parity;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.authlib.GameProfile;
import com.mojang.authlib.services.ServicesKeySet;
import com.mojang.serialization.Lifecycle;
import io.netty.channel.embedded.EmbeddedChannel;
import java.io.IOException;
import java.net.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.UUID;
import java.util.function.BooleanSupplier;
import net.minecraft.core.BlockPos;
import net.minecraft.SharedConstants;
import net.minecraft.SystemReport;
import net.minecraft.commands.Commands;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.UUIDUtil;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.Connection;
import net.minecraft.network.chat.ChatType;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.OutgoingChatMessage;
import net.minecraft.network.chat.Style;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.Services;
import net.minecraft.server.WorldLoader;
import net.minecraft.server.WorldStem;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.progress.LoggingLevelLoadListener;
import net.minecraft.server.network.CommonListenerCookie;
import net.minecraft.server.notifications.EmptyNotificationService;
import net.minecraft.server.notifications.NotificationManager;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionSet;
import net.minecraft.server.players.NameAndId;
import net.minecraft.server.players.PlayerList;
import net.minecraft.server.players.ProfileResolver;
import net.minecraft.server.players.UserNameToIdResolver;
import net.minecraft.util.Util;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.util.debugchart.LocalSampleLogger;
import net.minecraft.util.debugchart.SampleLogger;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.WorldDimensions;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.storage.LevelDataAndDimensions;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.world.level.storage.PrimaryLevelData;

/**
 * Runs command scripts through vanilla's real command implementations on a
 * stand-in integrated server (a real MinecraftServer with a generated world
 * and one single-player owner), recording every system and chat message the
 * player receives as styled spans together with the results reported to the
 * player's CommandResultCallback. Shared by tools/test_command_exec_parity.py.
 *
 *   VanillaCommandExecOracle <universe> <script.jsonl> <out.jsonl> <seed>
 */
public final class VanillaCommandExecOracle {
    static final List<JsonArray> messages = new ArrayList<>();

    static final class Resolver implements ProfileResolver, UserNameToIdResolver {
        @Override public Optional<GameProfile> fetchByName(String name) { return Optional.empty(); }
        @Override public Optional<GameProfile> fetchById(UUID id) { return Optional.empty(); }
        @Override public void add(NameAndId nameAndId) {}
        @Override public Optional<NameAndId> get(String name) { return Optional.empty(); }
        @Override public Optional<NameAndId> get(UUID id) { return Optional.empty(); }
        @Override public void resolveOfflineUsers(boolean value) {}
        @Override public void save() {}
    }

    static final class Server extends MinecraftServer {
        final Path script;
        final Path output;
        final LocalSampleLogger sampleLogger = new LocalSampleLogger(4);
        boolean ran;

        static Server create(Thread thread, LevelStorageSource.LevelStorageAccess access, PackRepository packs, long seed, Path script, Path output) throws Exception {
            packs.reload();
            WorldDataConfiguration config = WorldDataConfiguration.DEFAULT;
            LevelSettings settings = new LevelSettings("Harness", GameType.SURVIVAL, LevelSettings.DifficultySettings.DEFAULT, true, config);
            WorldLoader.PackConfig packConfig = new WorldLoader.PackConfig(packs, config, false, true);
            WorldLoader.InitConfig init = new WorldLoader.InitConfig(packConfig, Commands.CommandSelection.INTEGRATED, LevelBasedPermissionSet.OWNER);
            boolean flat = Boolean.getBoolean("horizon.parity.flat");
            WorldOptions options = new WorldOptions(seed, !flat, false);
            WorldStem stem = Util.<WorldStem>blockUntilDone(executor -> WorldLoader.load(init, context -> {
                Registry<LevelStem> noDatapackDimensions = new MappedRegistry<>(Registries.LEVEL_STEM, Lifecycle.stable()).freeze();
                WorldDimensions dimensions = context.datapackWorldRegistries().lookupOrThrow(Registries.WORLD_PRESET)
                    .getOrThrow(flat ? WorldPresets.FLAT : WorldPresets.NORMAL).value().createWorldDimensions();
                WorldDimensions.Complete complete = dimensions.bake(noDatapackDimensions);
                PrimaryLevelData data = new PrimaryLevelData(settings, complete.specialWorldProperty(), complete.lifecycle());
                return new WorldLoader.DataLoadOutput<>(
                    new LevelDataAndDimensions.WorldDataAndGenSettings(data, new WorldGenSettings(options, dimensions)),
                    complete.dimensionsRegistryAccess());
            }, WorldStem::new, Util.backgroundExecutor(), executor)).get();
            return new Server(thread, access, packs, stem, script, output);
        }

        Server(Thread thread, LevelStorageSource.LevelStorageAccess access, PackRepository packs, WorldStem stem, Path script, Path output) {
            super(thread, access, packs, stem, Optional.of(new GameRules(WorldDataConfiguration.DEFAULT.enabledFeatures())), Proxy.NO_PROXY,
                DataFixers.getDataFixer(), new Services(null, ServicesKeySet.EMPTY, null, new Resolver(), new Resolver()),
                LoggingLevelLoadListener.forDedicatedServer(), false, new NotificationManager());
            this.script = script;
            this.output = output;
        }

        @Override protected boolean initServer() {
            this.setPlayerList(new PlayerList(this, this.registries(), this.playerDataStorage, new EmptyNotificationService()) {});
            if (Boolean.getBoolean("horizon.loadedFixture")) {
                this.getPlayerList().setViewDistance(2);
                this.getPlayerList().setSimulationDistance(2);
            }
            this.loadLevel();
            if (Boolean.getBoolean("horizon.loadedFixture")) {
                this.getPlayerList().setViewDistance(2);
                this.getPlayerList().setSimulationDistance(2);
            }
            return true;
        }

        @Override protected void tickServer(BooleanSupplier haveTime) {
            if (ran) {
                return;
            }
            ran = true;
            try {
                runScript();
                if (Boolean.getBoolean("horizon.parity.flat")) saveEverything(true, true, true);
            } catch (Throwable t) {
                t.printStackTrace();
                Runtime.getRuntime().halt(2);
            }
            System.out.flush();
            Runtime.getRuntime().halt(0);
        }

        ServerPlayer player;

        // Execute the actual private arithmetic method on unregistered orbs in
        // this real server level. No world tick or live scene actor is added.
        void writeOrbWaterReference() throws IOException {
            String input = System.getProperty("horizon.orbWaterArithmetic");
            if (input == null) return;
            JsonArray result = new JsonArray();
            try {
                var method = net.minecraft.world.entity.ExperienceOrb.class.getDeclaredMethod("setUnderwaterMovement");
                method.setAccessible(true);
                for (var entry : JsonParser.parseString(Files.readString(Path.of(input))).getAsJsonArray()) {
                    JsonArray p = entry.getAsJsonArray();
                    var orb = new net.minecraft.world.entity.ExperienceOrb(net.minecraft.world.entity.EntityTypes.EXPERIENCE_ORB, player.level());
                    orb.setDeltaMovement(p.get(0).getAsDouble(), p.get(1).getAsDouble(), p.get(2).getAsDouble());
                    method.invoke(orb);
                    var v = orb.getDeltaMovement();
                    JsonObject row = new JsonObject(); row.add("input", p);
                    JsonArray bits = new JsonArray(); JsonArray values = new JsonArray();
                    for (double n : new double[] {v.x(), v.y(), v.z()}) {
                        bits.add(Long.toHexString(Double.doubleToRawLongBits(n))); values.add(n);
                    }
                    row.add("bits", bits); row.add("motion", values); result.add(row);
                }
            } catch (ReflectiveOperationException error) {
                throw new IOException("orb underwater reference failed", error);
            }
            Files.writeString(output.resolveSibling("orb-water-reference.json"), result.toString());
        }

        void runScript() throws IOException {
            ServerLevel level = this.overworld();
            GameProfile profile = new GameProfile(UUIDUtil.createOfflinePlayerUUID("Player"), "Player");
            CommonListenerCookie cookie = CommonListenerCookie.createInitial(profile, false);
            player = new ServerPlayer(this, level, profile, ClientInformation.createDefault()) {
                @Override public void sendSystemMessage(Component message, boolean overlay) {
                    if (!overlay) {
                        messages.add(spans(message));
                    }
                }
                @Override public void sendChatMessage(OutgoingChatMessage message, boolean filtered, ChatType.Bound chatType) {
                    messages.add(spans(chatType.decorate(message.content())));
                }
            };
            Connection connection = new Connection(PacketFlow.SERVERBOUND);
            EmbeddedChannel channel = new EmbeddedChannel(connection);
            JsonObject login = new JsonObject();
            if (Boolean.getBoolean("horizon.loadedFixture")) {
                channel.pipeline().addLast(new io.netty.channel.ChannelOutboundHandlerAdapter() {
                    @Override public void write(io.netty.channel.ChannelHandlerContext context, Object message,
                            io.netty.channel.ChannelPromise promise) throws Exception {
                        if (message instanceof net.minecraft.network.protocol.game.ClientboundLoginPacket packet) {
                            login.addProperty("view_distance", packet.chunkRadius());
                            login.addProperty("simulation_distance", packet.simulationDistance());
                        }
                        super.write(context, message, promise);
                    }
                });
            }
            this.getPlayerList().placeNewPlayer(connection, player, cookie);
            if (Boolean.getBoolean("horizon.loadedFixture")) {
                new net.minecraft.network.protocol.game.ServerboundPlayerLoadedPacket().handle(player.connection);
            }
            settle();
            if (Boolean.getBoolean("horizon.loadedFixture")) {
                login.addProperty("server_view", this.getPlayerList().getViewDistance());
                login.addProperty("server_simulation", this.getPlayerList().getSimulationDistance());
                login.addProperty("client_loaded", player.connection.hasClientLoaded());
                Files.writeString(output.resolveSibling("client_config.json"), login.toString());
            }
            writeOrbWaterReference();
            StringBuilder out = new StringBuilder();
            Gson gson = new Gson();
            for (String line : Files.readAllLines(script, StandardCharsets.UTF_8)) {
                if (line.isBlank()) {
                    continue;
                }
                JsonObject op = JsonParser.parseString(line).getAsJsonObject();
                switch (op.get("op").getAsString()) {
                    case "cmd" -> {
                        String command = op.get("cmd").getAsString();
                        messages.clear();
                        List<JsonArray> results = new ArrayList<>();
                        var source = player.createCommandSourceStack().withCallback((success, value) -> {
                            JsonArray r = new JsonArray();
                            r.add(success);
                            r.add(value);
                            results.add(r);
                        });
                        this.getCommands().performPrefixedCommand(source, command);
                        if (!op.has("record") || op.get("record").getAsBoolean()) {
                            JsonObject record = new JsonObject();
                            record.addProperty("cmd", command);
                            JsonArray m = new JsonArray();
                            messages.forEach(m::add);
                            record.add("messages", m);
                            JsonArray r = new JsonArray();
                            results.forEach(r::add);
                            record.add("results", r);
                            out.append(gson.toJson(record)).append('\n');
                        }
                    }
                    case "server_distances" -> {
                        JsonObject record = new JsonObject();
                        record.addProperty("cmd", "#server_distances");
                        record.add("messages", new JsonArray());
                        JsonArray values = new JsonArray();
                        for (int n : new int[] {getPlayerList().getViewDistance(), getPlayerList().getSimulationDistance()}) {
                            JsonArray value = new JsonArray(); value.add(true); value.add(n); values.add(value);
                        }
                        record.add("results", values); out.append(gson.toJson(record)).append('\n');
                    }
                    case "chunk_ready" -> {
                        JsonArray p = op.getAsJsonArray("chunk");
                        int x = p.get(0).getAsInt(), z = p.get(1).getAsInt();
                        var chunk = player.level().getChunkSource().getChunkNow(x, z);
                        boolean[] ready = {chunk != null, chunk != null && chunk.isLightCorrect(),
                            player.level().areEntitiesLoaded(new net.minecraft.world.level.ChunkPos(x, z).pack())};
                        JsonObject record = new JsonObject(); record.addProperty("cmd", "#chunk_ready " + x + " " + z);
                        record.add("messages", new JsonArray()); JsonArray values = new JsonArray();
                        for (boolean flag : ready) {JsonArray value = new JsonArray(); value.add(true); value.add(flag ? 1 : 0); values.add(value);}
                        record.add("results", values); out.append(gson.toJson(record)).append('\n');
                    }
                    case "wait_chunks" -> {
                        long deadline = System.nanoTime() + 180_000_000_000L;
                        while (true) {
                            tick(); boolean ready = true;
                            for (var target : op.getAsJsonArray("chunks")) {
                                var p = target.getAsJsonArray(); int x = p.get(0).getAsInt(), z = p.get(1).getAsInt();
                                var chunk = player.level().getChunkSource().getChunkNow(x, z);
                                ready &= chunk != null && chunk.isLightCorrect()
                                    && player.level().areEntitiesLoaded(new net.minecraft.world.level.ChunkPos(x, z).pack());
                            }
                            if (ready) break;
                            if (System.nanoTime() > deadline) throw new IllegalStateException("initial target readiness timed out");
                            try {Thread.sleep(2);} catch (InterruptedException error) {throw new RuntimeException(error);}
                        }
                    }
                    case "weather_state" -> {
                        // Read-only prerequisites for named weather predicates.
                        var data = level.getWeatherData();
                        JsonArray results = new JsonArray();
                        for (int value : new int[] {data.isRaining() ? 1 : 0,
                                data.isThundering() ? 1 : 0, data.getClearWeatherTime(),
                                data.getRainTime(), data.getThunderTime()}) {
                            JsonArray result = new JsonArray();
                            result.add(true);
                            result.add(value);
                            results.add(result);
                        }
                        JsonObject record = new JsonObject();
                        record.addProperty("cmd", "#weather_state " + op.get("label").getAsString());
                        record.add("messages", new JsonArray());
                        record.add("results", results);
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "player_loop" -> {
                        int n=op.get("n").getAsInt();
                        int before=player.getStats().getValue(net.minecraft.stats.Stats.CUSTOM.get(net.minecraft.stats.Stats.PLAY_TIME));
                        messages.clear();
                        for (int i=0;i<n;i++) { tick(); player.connection.tick(); }
                        int delta=player.getStats().getValue(net.minecraft.stats.Stats.CUSTOM.get(net.minecraft.stats.Stats.PLAY_TIME))-before;
                        if (delta!=n) throw new IllegalStateException("player loop did not advance play_time: "+delta+"/"+n);
                        JsonObject record=new JsonObject();record.addProperty("cmd","#player_loop "+op.get("label").getAsString());
                        JsonArray captured=new JsonArray();messages.forEach(captured::add);
                        record.add("messages",captured);record.add("results",gson.toJsonTree(new Object[][]{{true,delta}}));
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "player_state" -> {
                        try {
                            var field=net.minecraft.server.PlayerAdvancements.class.getDeclaredField("progress");field.setAccessible(true);
                            var progress=(java.util.Map<?,?>)field.get(player.getAdvancements());
                            var loaded=new java.util.ArrayList<String>();var done=new java.util.TreeMap<String,Boolean>();
                            for (var a:getAdvancements().getAllAdvancements()) {
                                String id=a.id().toString();if (!id.startsWith("h311:")) continue;
                                loaded.add(id);var value=(net.minecraft.advancements.AdvancementProgress)progress.get(a);
                                done.put(id,value!=null && value.isDone());
                            }
                            java.util.Collections.sort(loaded);
                            var poison=player.getEffect(net.minecraft.world.effect.MobEffects.POISON);
                            JsonObject state=new JsonObject();state.addProperty("health",player.getHealth());
                            state.addProperty("poison_ticks",poison==null?0:poison.getDuration());state.add("loaded",gson.toJsonTree(loaded));state.add("done",gson.toJsonTree(done));
                            JsonObject record=new JsonObject();record.addProperty("cmd","#player_state "+op.get("label").getAsString());
                            record.add("messages",new JsonArray());record.add("results",new JsonArray());record.add("state",state);
                            out.append(gson.toJson(record)).append('\n');
                        } catch (ReflectiveOperationException e) {throw new IllegalStateException(e);}
                    }
                    case "tick" -> {
                        int n = op.has("n") ? op.get("n").getAsInt() : 1;
                        messages.clear();
                        // Answers computed off-thread (profile lookups) reach the
                        // server's task queue before the next tick.
                        // (Other queued tasks may be pending already, so give
                        // the stub resolvers a fixed moment rather than
                        // waiting for the queue to fill.)
                        if (op.has("record")) {
                            try { Thread.sleep(300); } catch (InterruptedException e) { throw new RuntimeException(e); }
                        }
                        long pacedStart = System.nanoTime();
                        for (int i = 0; i < n; i++) {
                            tick();
                            if (op.has("pace_ms")) {
                                long left = op.get("pace_ms").getAsLong() * (i + 1) * 1_000_000L - (System.nanoTime() - pacedStart);
                                if (left > 0) try {Thread.sleep(left / 1_000_000L, (int)(left % 1_000_000L));}
                                catch (InterruptedException error) {throw new RuntimeException(error);}
                            }
                        }
                        if (op.has("pace_ms")) System.err.println("harness paced wait: " + n + " ticks, " + ((System.nanoTime() - pacedStart) / 1_000_000L) + " ms");
                        if (op.has("record") && op.get("record").getAsBoolean()) {
                            JsonObject record = new JsonObject();
                            record.addProperty("cmd", "#tick");
                            JsonArray m = new JsonArray();
                            messages.forEach(m::add);
                            record.add("messages", m);
                            record.add("results", new JsonArray());
                            out.append(gson.toJson(record)).append('\n');
                        }
                    }
                    case "dragon_contacts" -> {
                        // Optional fixture: the shared runner still compiles on its own.
                        try {
                            Class<?> fixture = Class.forName("dev.horizonlab.parity.VanillaDragonContactsOracle");
                            var method = fixture.getDeclaredMethod("run", ServerLevel.class);
                            out.append(gson.toJson(method.invoke(null, level))).append('\n');
                        } catch (ReflectiveOperationException e) {
                            throw new IllegalStateException(e);
                        }
                    }
                    case "settle" -> {
                        settle();
                        if (op.has("required_chunks")) {
                            long deadline = System.nanoTime() + 120_000_000_000L;
                            while (true) {
                                boolean ready = true;
                                for (var element : op.getAsJsonArray("required_chunks")) {
                                    var pair = element.getAsJsonArray();
                                    ready &= player.level().areEntitiesActuallyLoadedAndTicking(
                                        new net.minecraft.world.level.ChunkPos(pair.get(0).getAsInt(), pair.get(1).getAsInt()));
                                }
                                if (ready) break;
                                if (System.nanoTime() > deadline) throw new IllegalStateException("required chunks did not load");
                                tick();
                                Thread.yield();
                            }
                        }
                    }
                    case "use_item" -> {
                        player.connection.handleUseItem(new net.minecraft.network.protocol.game.ServerboundUseItemPacket(
                            net.minecraft.world.InteractionHand.MAIN_HAND, op.get("sequence").getAsInt(),
                            op.get("yaw").getAsFloat(), op.get("pitch").getAsFloat()));
                        try {
                            var ack = net.minecraft.server.network.ServerGamePacketListenerImpl.class.getDeclaredField("ackBlockChangesUpTo");
                            ack.setAccessible(true);
                            if (ack.getInt(player.connection) < op.get("sequence").getAsInt())
                                throw new IllegalStateException("UseItem was not acknowledged");
                        } catch (ReflectiveOperationException e) {
                            throw new IllegalStateException(e);
                        }
                    }
                    case "use_block" -> {
                        JsonArray p = op.getAsJsonArray("pos");
                        BlockPos pos = new BlockPos(p.get(0).getAsInt(), p.get(1).getAsInt(), p.get(2).getAsInt());
                        int face = op.has("face") ? op.get("face").getAsInt() : 1;
                        var hit = new net.minecraft.world.phys.BlockHitResult(
                            new net.minecraft.world.phys.Vec3(pos.getX() + 0.5, pos.getY() + 1.0, pos.getZ() + 0.5),
                            net.minecraft.core.Direction.from3DDataValue(face), pos, false);
                        player.gameMode.useItemOn(player, player.level(), player.getMainHandItem(),
                            net.minecraft.world.InteractionHand.MAIN_HAND, hit);
                    }
                    case "break" -> {
                        JsonArray pos = op.getAsJsonArray("pos");
                        player.gameMode.destroyBlock(new BlockPos(pos.get(0).getAsInt(), pos.get(1).getAsInt(), pos.get(2).getAsInt()));
                    }
                    case "portal" -> portal();
                    case "destroy_progress" -> {
                        JsonArray coords = op.getAsJsonArray("pos");
                        BlockPos pos = new BlockPos(coords.get(0).getAsInt(), coords.get(1).getAsInt(), coords.get(2).getAsInt());
                        // The fixture server does not tick its standalone player. Run
                        // LivingEntity's actual equipment refresh before observing mining.
                        try {
                            var refresh = net.minecraft.world.entity.LivingEntity.class.getDeclaredMethod("detectEquipmentUpdates");
                            refresh.setAccessible(true);
                            refresh.invoke(player);
                        } catch (ReflectiveOperationException error) {
                            throw new IllegalStateException(error);
                        }
                        player.setOnGround(true);
                        float progress = player.level().getBlockState(pos).getDestroyProgress(player, player.level(), pos);
                        JsonObject record = new JsonObject();
                        record.addProperty("cmd", "#destroy_progress");
                        record.add("messages", new JsonArray());
                        JsonArray value = new JsonArray();
                        value.add(true);
                        value.add(Float.floatToRawIntBits(progress));
                        JsonArray results = new JsonArray();
                        results.add(value);
                        record.add("results", results);
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "falling" -> {
                        String dimension = op.get("dimension").getAsString();
                        String block = op.get("block").getAsString();
                        boolean noGravity = op.get("no_gravity").getAsBoolean();
                        var key = net.minecraft.resources.ResourceKey.create(Registries.DIMENSION,
                            net.minecraft.resources.Identifier.parse(dimension));
                        var portalLevel = this.getLevel(key);
                        var inspected = new ArrayList<net.minecraft.world.entity.item.FallingBlockEntity>();
                        for (var entity : portalLevel.getAllEntities()) {
                            if (entity instanceof net.minecraft.world.entity.item.FallingBlockEntity f
                                && net.minecraft.core.registries.BuiltInRegistries.BLOCK
                                    .getKey(f.getBlockState().getBlock()).toString().equals(block)
                                && f.isNoGravity() == noGravity) inspected.add(f);
                        }
                        int count = inspected.size();
                        if (op.has("discard") && op.get("discard").getAsBoolean())
                            inspected.forEach(f -> f.discard());
                        JsonObject record = new JsonObject();
                        record.addProperty("cmd", "#falling " + dimension + " " + block + " " + noGravity);
                        record.add("messages", new JsonArray());
                        JsonArray result = new JsonArray();
                        result.add(true);
                        result.add(count);
                        JsonArray results = new JsonArray();
                        results.add(result);
                        record.add("results", results);
                        out.append(gson.toJson(record)).append('\n');
                    }
                    default -> throw new IllegalArgumentException(line);
                }
            }
            Files.writeString(output, out.toString(), StandardCharsets.UTF_8);
        }

        /** One iteration of MinecraftServer.runServer: the delayed tick profiler start, then the tick. */
        void tick() {
            try {
                var delay = MinecraftServer.class.getDeclaredField("debugCommandProfilerDelayStart");
                delay.setAccessible(true);
                if (delay.getBoolean(this)) {
                    delay.setBoolean(this, false);
                    Class<?> profilerClass = Class.forName("net.minecraft.server.MinecraftServer$TimeProfiler");
                    var ctor = profilerClass.getDeclaredConstructor(long.class, int.class);
                    ctor.setAccessible(true);
                    var field = MinecraftServer.class.getDeclaredField("debugCommandProfiler");
                    field.setAccessible(true);
                    field.set(this, ctor.newInstance(Util.getNanos(), this.getTickCount()));
                }
            } catch (ReflectiveOperationException e) {
                throw new RuntimeException(e);
            }
            drain();
            super.tickServer(() -> false);
            // The player's network listener ticks it each server tick
            // (ServerGamePacketListenerImpl.tickPlayer); this one has no
            // listening connection. Its movement is the client's, so of
            // that tick only apply changed attributes (waypoint range...).
            if (player != null) {
                try {
                    var refresh = net.minecraft.world.entity.LivingEntity.class.getDeclaredMethod("refreshDirtyAttributes");
                    refresh.setAccessible(true);
                    refresh.invoke(player);
                } catch (ReflectiveOperationException e) {
                    throw new RuntimeException(e);
                }
            }
            drain();
        }

        /** The server's and every level's chunk task queues (runServer's waits run them). */
        void drain() {
            boolean more = true;
            while (more) {
                more = false;
                this.runAllTasks();
                for (ServerLevel level : this.getAllLevels()) {
                    while (level.getChunkSource().pollTask()) {
                        more = true;
                    }
                }
            }
        }

        /** Force-load the chunks around the player (Horizon waits for its streamed terrain). */
        /**
         * Entity.handlePortal once the player's portal process finished in the
         * nether portal it stands in (the oracle does not tick the player).
         */
        void portal() {
            ServerLevel level = player.level();
            net.minecraft.core.BlockPos entry = player.blockPosition();
            if (!level.getBlockState(entry).is(net.minecraft.world.level.block.Blocks.NETHER_PORTAL)) {
                throw new IllegalStateException("portal: the player is not in a nether portal");
            }
            var block = (net.minecraft.world.level.block.NetherPortalBlock) net.minecraft.world.level.block.Blocks.NETHER_PORTAL;
            var transition = block.getPortalDestination(level, player, entry);
            if (transition != null) {
                player.teleportToPortalDestination(level, transition);
            }
        }

        void settle() {
            ServerLevel level = player.level();
            if (Boolean.getBoolean("horizon.loadedFixture")) {
                // Use the actual player tickets; no permanent fixture DRAGON tickets.
                level.getChunkSource().move(player);
                long deadline = System.nanoTime() + 120_000_000_000L;
                while (true) {
                    tick(); boolean ready = true;
                    for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) {
                        var pos = new net.minecraft.world.level.ChunkPos(player.chunkPosition().x() + dx, player.chunkPosition().z() + dz);
                        ready &= level.getChunkSource().getChunkNow(pos.x(), pos.z()) != null && level.areEntitiesLoaded(pos.pack());
                    }
                    if (ready) return;
                    if (System.nanoTime() > deadline) throw new IllegalStateException("real player tickets did not load");
                    try {Thread.sleep(5);} catch (InterruptedException error) {throw new RuntimeException(error);}
                }
            }
            int cx = player.chunkPosition().x(), cz = player.chunkPosition().z();
            for (int dz = -2; dz <= 2; dz++) {
                for (int dx = -2; dx <= 2; dx++) {
                    // An entity-ticking ticket that, unlike setChunkForced,
                    // is not a force-load /forceload would report.
                    level.getChunkSource().addTicket(
                        new net.minecraft.server.level.Ticket(net.minecraft.server.level.TicketType.DRAGON, net.minecraft.server.level.ChunkMap.FORCED_TICKET_LEVEL),
                        new net.minecraft.world.level.ChunkPos(cx + dx, cz + dz));
                    level.getChunk(cx + dx, cz + dz);
                }
            }
            // Entity sections become visible once their (asynchronous) load
            // finished and the chunk entity-ticks. That load runs on other
            // threads, so wait by the clock, not by a tick count: 400 ticks
            // (~2 s) ran out on a loaded runner and failed main's parity
            // (2026-10-29).
            long deadline = System.nanoTime() + 120_000_000_000L;
            for (int i = 0; System.nanoTime() < deadline; i++) {
                tick();
                boolean ready = true;
                for (int dz = -2; dz <= 2 && ready; dz++) {
                    for (int dx = -2; dx <= 2 && ready; dx++) {
                        ready = level.areEntitiesActuallyLoadedAndTicking(new net.minecraft.world.level.ChunkPos(cx + dx, cz + dz));
                    }
                }
                if (ready && i >= 5) {
                    return;
                }
                try { Thread.sleep(5); } catch (InterruptedException e) { throw new RuntimeException(e); }
            }
            throw new IllegalStateException("entities around the player did not load");
        }

        static JsonArray spans(Component message) {
            JsonArray out = new JsonArray();
            message.visit((style, text) -> {
                if (text.isEmpty()) {
                    return Optional.empty();
                }
                JsonArray key = styleKey(style);
                if (!out.isEmpty()) {
                    JsonArray last = out.get(out.size() - 1).getAsJsonArray();
                    if (last.get(1).equals(key)) {
                        last.set(0, new com.google.gson.JsonPrimitive(last.get(0).getAsString() + text));
                        return Optional.empty();
                    }
                }
                JsonArray span = new JsonArray();
                span.add(text);
                span.add(key);
                out.add(span);
                return Optional.empty();
            }, Style.EMPTY);
            return out;
        }

        static JsonArray styleKey(Style style) {
            JsonArray key = new JsonArray();
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

        @Override public LevelBasedPermissionSet getProfilePermissions(NameAndId nameAndId) { return LevelBasedPermissionSet.OWNER; }
        @Override protected SampleLogger getTickTimeLogger() { return sampleLogger; }
        @Override public boolean isTickTimeLoggingEnabled() { return false; }
        @Override public SystemReport fillServerSystemReport(SystemReport report) { return report; }
        @Override public boolean isHardcore() { return false; }
        @Override public LevelBasedPermissionSet operatorUserPermissions() { return LevelBasedPermissionSet.GAMEMASTER; }
        @Override public PermissionSet getFunctionCompilationPermissions() { return LevelBasedPermissionSet.GAMEMASTER; }
        @Override public boolean shouldRconBroadcast() { return true; }
        @Override public boolean isDedicatedServer() { return false; }
        @Override public int getRateLimitPacketsPerSecond() { return 0; }
        @Override public int getCommandSpamThresholdSeconds() { return 0; }
        @Override public int getChatSpamThresholdSeconds() { return 0; }
        @Override public boolean useNativeTransport() { return false; }
        @Override public boolean isPublished() { return false; }
        @Override public boolean shouldInformAdmins() { return true; }
        @Override public boolean isSingleplayerOwner(NameAndId nameAndId) { return nameAndId.name().equalsIgnoreCase("Player"); }
        @Override public int getMaxPlayers() { return 8; }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Path universe = Path.of(args[0]);
        Path script = Path.of(args[1]).toAbsolutePath();
        Path output = Path.of(args[2]).toAbsolutePath();
        long seed = Long.parseLong(args[3]);
        Files.createDirectories(universe);
        LevelStorageSource.LevelStorageAccess access = LevelStorageSource.createDefault(universe).createAccess("harness");
        PackRepository packs = ServerPacksSource.createPackRepository(access);
        MinecraftServer.spin(thread -> {
            try {
                return Server.create(thread, access, packs, seed, script, output);
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        });
    }
}
