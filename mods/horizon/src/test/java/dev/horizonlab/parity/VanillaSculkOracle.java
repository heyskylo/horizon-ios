package dev.horizonlab.parity;

import com.mojang.authlib.GameProfile;
import com.mojang.serialization.Lifecycle;
import io.netty.channel.embedded.EmbeddedChannel;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.net.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.UUID;
import java.util.function.BooleanSupplier;
import net.minecraft.SharedConstants;
import net.minecraft.SystemReport;
import net.minecraft.commands.Commands;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.gametest.framework.GameTestServer;
import net.minecraft.gizmos.GizmoCollector;
import net.minecraft.gizmos.Gizmos;
import net.minecraft.network.Connection;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.network.protocol.game.ClientboundLevelEventPacket;
import net.minecraft.network.protocol.game.ClientboundSoundEntityPacket;
import net.minecraft.network.protocol.game.ClientboundSoundPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.Services;
import net.minecraft.server.WorldLoader;
import net.minecraft.server.WorldStem;
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
import net.minecraft.util.Util;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.util.debugchart.LocalSampleLogger;
import net.minecraft.util.debugchart.SampleLogger;
import net.minecraft.world.Difficulty;
import net.minecraft.world.clock.WorldClocks;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.monster.warden.WardenSpawnTracker;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.DataPackConfig;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.block.SculkSpreader;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.SculkCatalystBlockEntity;
import net.minecraft.world.level.block.entity.SculkSensorBlockEntity;
import net.minecraft.world.level.block.entity.SculkShriekerBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gameevent.vibrations.VibrationInfo;
import net.minecraft.world.level.gameevent.vibrations.VibrationSelector;
import net.minecraft.world.level.gameevent.vibrations.VibrationSystem;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldDimensions;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.storage.LevelDataAndDimensions;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.world.level.storage.PrimaryLevelData;
import net.minecraft.world.phys.Vec3;

/**
 * Sculk oracle: a real (headless, flat, structure-free) vanilla server level, as in
 * {@link VanillaMobAiOracle}. Each scenario builds blocks, places the player and entities, and
 * then runs its ticks the way ServerLevel.tick orders them (game time, scheduled block and fluid
 * ticks, block events, the scenario's scripted game events and actions, the player's warden spawn
 * tracker and the entities, then the block entities) while the rest of the level stands still. After
 * every tick it prints what changed at the watched blocks (state and the sculk block entity:
 * vibration, selector, frequency, warning level, charge cursors), the sounds and level events,
 * the player's warning level and darkness, entities that appeared, and the level random.
 * Used by tools/test_sculk_parity.py.
 */
public final class VanillaSculkOracle {
    static final List<Scenario> SCENARIOS = new ArrayList<>();
    static PrintWriter out;
    static final List<String> LOG = new ArrayList<>();

    /** Draws of the level random with their callers (SCULK_TRACE=1). */
    static final class TracingRandom extends LegacyRandomSource {
        TracingRandom(long seed) { super(seed); }
        @Override
        public int next(int bits) {
            int v = super.next(bits);
            StringBuilder sb = new StringBuilder("trace " + bits + ":");
            int n = 0;
            for (StackTraceElement e : new Throwable().getStackTrace()) {
                String c = e.getClassName();
                if (c.contains("RandomSource") || c.contains("TracingRandom") || c.contains("Mth")) continue;
                sb.append(' ').append(c.substring(c.lastIndexOf('.') + 1)).append('.').append(e.getMethodName()).append(':').append(e.getLineNumber());
                if (++n == 6) break;
            }
            LOG.add(sb.toString());
            return v;
        }
    }

    static final class MobSpec {
        String type;
        double x, y, z;
        float yaw;
        long seed;
        List<String> flags = new ArrayList<>();
    }

    /** A scripted action at a tick. */
    static final class Action {
        int tick;
        String[] words;
    }

    static final class Scenario {
        String name;
        int ticks;
        int originX, originZ;
        long levelSeed = 0;
        Difficulty difficulty = Difficulty.NORMAL;
        boolean wardens = true;
        List<String[]> blocks = new ArrayList<>();
        double[] player;
        String mode = "survival";
        boolean sneak;
        int warning = -1;
        List<MobSpec> mobs = new ArrayList<>();
        List<int[]> watch = new ArrayList<>();
        List<int[]> regions = new ArrayList<>();
        List<Action> actions = new ArrayList<>();
    }

    static void parse(Path path) throws Exception {
        Scenario s = null;
        for (String raw : Files.readAllLines(path)) {
            String line = raw.strip();
            if (line.isEmpty() || line.startsWith("#")) continue;
            String[] f = line.split("\\s+");
            switch (f[0]) {
                case "scenario" -> {
                    s = new Scenario();
                    s.name = f[1];
                    for (int i = 2; i < f.length; i++) {
                        String[] kv = f[i].split("=", 2);
                        switch (kv[0]) {
                            case "ticks" -> s.ticks = Integer.parseInt(kv[1]);
                            case "levelseed" -> s.levelSeed = Long.parseLong(kv[1]);
                            case "difficulty" -> s.difficulty = Difficulty.byName(kv[1]);
                            case "nowardens" -> s.wardens = false;
                            default -> throw new IllegalArgumentException(line);
                        }
                    }
                }
                case "origin" -> { s.originX = Integer.parseInt(f[1]); s.originZ = Integer.parseInt(f[2]); }
                case "fill", "block" -> s.blocks.add(f);
                case "player" -> {
                    s.player = new double[] {Double.parseDouble(f[1]), Double.parseDouble(f[2]), Double.parseDouble(f[3])};
                    for (int i = 4; i < f.length; i++) {
                        if (f[i].equals("sneak")) s.sneak = true;
                        else if (f[i].startsWith("warn=")) s.warning = Integer.parseInt(f[i].substring(5));
                        else s.mode = f[i];
                    }
                }
                case "mob" -> {
                    MobSpec m = new MobSpec();
                    m.type = f[1];
                    m.x = Double.parseDouble(f[2]);
                    m.y = Double.parseDouble(f[3]);
                    m.z = Double.parseDouble(f[4]);
                    m.yaw = Float.parseFloat(f[5]);
                    m.seed = Long.parseLong(f[6]);
                    for (int i = 7; i < f.length; i++) m.flags.add(f[i]);
                    s.mobs.add(m);
                }
                case "region" -> s.regions.add(new int[] {Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3]),
                    Integer.parseInt(f[4]), Integer.parseInt(f[5]), Integer.parseInt(f[6])});
                case "watch" -> s.watch.add(new int[] {Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3])});
                case "at" -> {
                    Action a = new Action();
                    a.tick = Integer.parseInt(f[1]);
                    a.words = java.util.Arrays.copyOfRange(f, 2, f.length);
                    s.actions.add(a);
                }
                case "end" -> { SCENARIOS.add(s); s = null; }
                default -> throw new IllegalArgumentException(line);
            }
        }
    }

    public static void main(String[] args) throws Exception {
        parse(Path.of(args[0]));
        out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])));
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Util.startTimerHackThread();
        Path universe = Files.createTempDirectory("horizon-sculk");
        LevelStorageSource.LevelStorageAccess access = LevelStorageSource.createDefault(universe).createAccess("sculkworld");
        PackRepository packs = ServerPacksSource.createPackRepository(access);
        MinecraftServer.spin(thread -> Server.create(thread, access, packs));
    }

    static String d(double v) { return Long.toHexString(Double.doubleToRawLongBits(v)); }
    static String f(float v) { return Integer.toHexString(Float.floatToRawIntBits(v)); }
    static String pos(BlockPos p) { return p.getX() + "," + p.getY() + "," + p.getZ(); }

    static Field field(Class<?> c, String name) throws Exception {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                Field fl = k.getDeclaredField(name);
                fl.setAccessible(true);
                return fl;
            } catch (NoSuchFieldException e) {
                // keep looking
            }
        }
        throw new NoSuchFieldException(name);
    }

    static Method method(Class<?> c, String name, Class<?>... types) throws Exception {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                Method m = k.getDeclaredMethod(name, types);
                m.setAccessible(true);
                return m;
            } catch (NoSuchMethodException e) {
                // keep looking
            }
        }
        throw new NoSuchMethodException(name);
    }

    static Object get(Object o, String name) throws Exception { return field(o.getClass(), name).get(o); }

    /**
     * A mob's own random, its sensors' first delays and its gates' shuffles (RunOne's ShufflingList)
     * are unseeded: seed what appeared, as Horizon does.
     */
    static void seedFresh(Mob mob, long seed) throws Exception {
        mob.getRandom().setSeed(seed);
        @SuppressWarnings("unchecked")
        Map<Object, net.minecraft.world.entity.ai.sensing.Sensor<?>> sensors =
            (Map<Object, net.minecraft.world.entity.ai.sensing.Sensor<?>>) get(mob.getBrain(), "sensors");
        for (var sensor : sensors.values()) sensor.randomlyDelayStart(mob.getRandom());
        @SuppressWarnings("unchecked")
        Map<Integer, Map<?, Set<net.minecraft.world.entity.ai.behavior.BehaviorControl<?>>>> byPriority =
            (Map<Integer, Map<?, Set<net.minecraft.world.entity.ai.behavior.BehaviorControl<?>>>>) get(mob.getBrain(), "availableBehaviorsByPriority");
        for (var byActivity : byPriority.values())
            for (var behaviors : byActivity.values())
                for (var behavior : behaviors) seedGate(behavior, seed);
    }

    static void seedGate(Object behavior, long seed) throws Exception {
        if (!(behavior instanceof net.minecraft.world.entity.ai.behavior.GateBehavior<?>)) return;
        var list = (net.minecraft.world.entity.ai.behavior.ShufflingList<?>)
            field(net.minecraft.world.entity.ai.behavior.GateBehavior.class, "behaviors").get(behavior);
        field(net.minecraft.world.entity.ai.behavior.ShufflingList.class, "random").set(list, new LegacyRandomSource(seed));
        for (Object child : list) seedGate(child, seed);
    }

    static long seedOf(Object random) throws Exception {
        return ((java.util.concurrent.atomic.AtomicLong) field(LegacyRandomSource.class, "seed").get(random)).get();
    }

    static final class Server extends MinecraftServer {
        static final FeatureFlagSet FEATURES = FeatureFlags.REGISTRY
            .allFlags()
            .subtract(FeatureFlagSet.of(FeatureFlags.REDSTONE_EXPERIMENTS, FeatureFlags.MINECART_IMPROVEMENTS));
        final LocalSampleLogger sampleLogger = new LocalSampleLogger(4);
        int scenario = 0;
        int phase = 0;
        int wait = 0;
        ServerPlayer player;
        Map<Entity, String> names = new HashMap<>();
        int[] forced;
        static final int RESET = 24;

        static Server create(Thread thread, LevelStorageSource.LevelStorageAccess access, PackRepository packs) {
            try {
                packs.reload();
                ArrayList<String> enabled = new ArrayList<>(packs.getAvailableIds());
                enabled.remove("vanilla");
                enabled.addFirst("vanilla");
                WorldDataConfiguration config = new WorldDataConfiguration(new DataPackConfig(enabled, List.of()), FEATURES);
                LevelSettings settings = new LevelSettings("Sculk Level", GameType.SURVIVAL, LevelSettings.DifficultySettings.DEFAULT, true, config);
                WorldLoader.PackConfig packConfig = new WorldLoader.PackConfig(packs, config, false, true);
                WorldLoader.InitConfig init = new WorldLoader.InitConfig(packConfig, Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.OWNER);
                WorldStem stem = Util.<WorldStem>blockUntilDone(executor -> WorldLoader.load(init, context -> {
                    Registry<LevelStem> none = new MappedRegistry<>(Registries.LEVEL_STEM, Lifecycle.stable()).freeze();
                    WorldDimensions dims = context.datapackWorldRegistries().lookupOrThrow(Registries.WORLD_PRESET)
                        .getOrThrow(WorldPresets.FLAT).value().createWorldDimensions();
                    WorldDimensions.Complete complete = dims.bake(none);
                    PrimaryLevelData data = new PrimaryLevelData(settings, complete.specialWorldProperty(), complete.lifecycle());
                    return new WorldLoader.DataLoadOutput<>(
                        new LevelDataAndDimensions.WorldDataAndGenSettings(data, new WorldGenSettings(new WorldOptions(0L, false, false), dims)),
                        complete.dimensionsRegistryAccess());
                }, WorldStem::new, Util.backgroundExecutor(), executor)).get();
                GameRules rules = new GameRules(FEATURES);
                rules.set(GameRules.SPAWN_MOBS, false, null);
                rules.set(GameRules.SPAWN_MONSTERS, false, null);
                rules.set(GameRules.SPAWN_PATROLS, false, null);
                rules.set(GameRules.SPAWN_PHANTOMS, false, null);
                rules.set(GameRules.SPAWN_WANDERING_TRADERS, false, null);
                rules.set(GameRules.ADVANCE_TIME, false, null);
                rules.set(GameRules.ADVANCE_WEATHER, false, null);
                rules.set(GameRules.RAIDS, false, null);
                rules.set(GameRules.RANDOM_TICK_SPEED, 0, null);
                Field nf = GameTestServer.class.getDeclaredField("NO_SERVICES");
                nf.setAccessible(true);
                return new Server(thread, access, packs, stem, rules, (Services) nf.get(null));
            } catch (Exception e) {
                e.printStackTrace();
                System.exit(2);
                throw new IllegalStateException(e);
            }
        }

        Server(Thread thread, LevelStorageSource.LevelStorageAccess access, PackRepository packs, WorldStem stem, GameRules rules, Services services) {
            super(thread, access, packs, stem, Optional.of(rules), Proxy.NO_PROXY, DataFixers.getDataFixer(), services,
                LoggingLevelLoadListener.forDedicatedServer(), false, new NotificationManager());
        }

        @Override
        protected boolean initServer() {
            this.setPlayerList(new PlayerList(this, this.registries(), this.playerDataStorage, new EmptyNotificationService()) {
                @Override
                public void broadcast(Player except, double x, double y, double z, double range, ResourceKey<Level> dim, Packet<?> packet) {
                    capture(packet);
                }
            });
            Gizmos.withCollector(GizmoCollector.NOOP);
            this.loadLevel();
            return true;
        }

        static void capture(Packet<?> packet) {
            if (packet instanceof ClientboundSoundPacket p) {
                LOG.add("sound " + p.getSound().value().location() + " " + (int) Math.floor(p.getX()) + "," + (int) Math.floor(p.getY()) + ","
                    + (int) Math.floor(p.getZ()) + " " + f(p.getVolume()) + " " + f(p.getPitch()));
            } else if (packet instanceof ClientboundSoundEntityPacket p) {
                LOG.add("sound " + p.getSound().value().location() + " entity " + f(p.getVolume()) + " " + f(p.getPitch()));
            } else if (packet instanceof ClientboundLevelEventPacket p) {
                LOG.add("level " + p.getType() + " " + pos(p.getPos()) + " " + p.getData());
                if (p.getType() == 2001) {
                    // LevelEventHandler: the block's break sound (the client plays it).
                    BlockState state = net.minecraft.world.level.block.Block.stateById(p.getData());
                    if (!state.isAir()) {
                        var sound = state.getSoundType();
                        LOG.add("sound " + sound.getBreakSound().location() + " " + pos(p.getPos()) + " " + f((sound.getVolume() + 1.0F) / 2.0F)
                            + " " + f(sound.getPitch() * 0.8F));
                    }
                }
            }
        }

        @Override
        protected void tickServer(BooleanSupplier haveTime) {
            try {
                step(haveTime);
            } catch (Throwable t) {
                t.printStackTrace();
                out.flush();
                System.exit(3);
            }
        }

        ServerLevel level() { return this.overworld(); }

        void step(BooleanSupplier haveTime) throws Exception {
            if (scenario >= SCENARIOS.size()) {
                out.close();
                Runtime.getRuntime().halt(0);
                return;
            }
            Scenario s = SCENARIOS.get(scenario);
            ServerLevel level = level();
            int cx = s.originX >> 4, cz = s.originZ >> 4;
            switch (phase) {
                case 0 -> {
                    // Every scenario at the same origin reuses the forced chunks (reset between scenarios).
                    if (forced == null || forced[0] != cx || forced[1] != cz) {
                        if (forced != null)
                            for (int dx = -4; dx <= 4; dx++)
                                for (int dz = -4; dz <= 4; dz++)
                                    level.setChunkForced(forced[0] + dx, forced[1] + dz, false);
                        for (int dx = -4; dx <= 4; dx++)
                            for (int dz = -4; dz <= 4; dz++)
                                level.setChunkForced(cx + dx, cz + dz, true);
                        forced = new int[] {cx, cz};
                    }
                    phase = 1;
                    super.tickServer(haveTime);
                }
                case 1 -> {
                    super.tickServer(haveTime);
                    boolean ready = true;
                    for (int dx = -4; dx <= 4 && ready; dx++)
                        for (int dz = -4; dz <= 4 && ready; dz++)
                            ready = level.isPositionEntityTicking(new BlockPos((cx + dx) * 16, -60, (cz + dz) * 16));
                    if (ready) {
                        // Built and run within one server tick: nothing ticks in between (light is
                        // not updated either; nothing sculk does reads it).
                        reset(s, level);
                        this.getWorldData().overworldData().setGameTime(1000L);
                        build(s, level);
                        run(s, level);
                        scenario++;
                        phase = 0;
                    }
                }
                default -> throw new IllegalStateException();
            }
        }

        /** Put the flat preset back around the origin: no blocks, block entities, ticks or entities left from the last scenario. */
        void reset(Scenario s, ServerLevel level) throws Exception {
            for (Entity e : allEntities(level)) if (!(e instanceof ServerPlayer)) e.discard();
            BlockState bedrock = net.minecraft.world.level.block.Blocks.BEDROCK.defaultBlockState();
            BlockState dirt = net.minecraft.world.level.block.Blocks.DIRT.defaultBlockState();
            BlockState grass = net.minecraft.world.level.block.Blocks.GRASS_BLOCK.defaultBlockState();
            BlockState air = net.minecraft.world.level.block.Blocks.AIR.defaultBlockState();
            int flags = 2 | 16 | 256 | 512;
            BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
            for (int x = s.originX - RESET; x <= s.originX + RESET; x++)
                for (int z = s.originZ - RESET; z <= s.originZ + RESET; z++)
                    for (int y = -64; y <= -30; y++) {
                        BlockState want = y == -64 ? bedrock : y < -61 ? dirt : y == -61 ? grass : air;
                        p.set(x, y, z);
                        if (level.getBlockState(p) != want) level.setBlock(p, want, flags);
                    }
            var box = new net.minecraft.world.level.levelgen.structure.BoundingBox(s.originX - RESET - 2, -64, s.originZ - RESET - 2,
                s.originX + RESET + 2, 0, s.originZ + RESET + 2);
            level.getBlockTicks().clearArea(box);
            level.getFluidTicks().clearArea(box);
            ((java.util.Collection<?>) get(level, "blockEvents")).clear();
        }

        /** The level's entities, copied (discarding while iterating leaves holes). */
        static List<Entity> allEntities(ServerLevel level) {
            List<Entity> all = new ArrayList<>();
            for (Entity e : level.getAllEntities()) if (e != null) all.add(e);
            return all;
        }

        static BlockState parseState(ServerLevel level, String text) throws Exception {
            return BlockStateParser.parseForBlock(level.holderLookup(Registries.BLOCK), text, false).blockState();
        }

        void build(Scenario s, ServerLevel level) throws Exception {
            for (String[] b : s.blocks) {
                if (b[0].equals("block")) {
                    level.setBlock(new BlockPos(Integer.parseInt(b[1]), Integer.parseInt(b[2]), Integer.parseInt(b[3])), parseState(level, b[4]), 2 | 16);
                } else {
                    BlockState state = parseState(level, b[7]);
                    int x0 = Integer.parseInt(b[1]), y0 = Integer.parseInt(b[2]), z0 = Integer.parseInt(b[3]);
                    int x1 = Integer.parseInt(b[4]), y1 = Integer.parseInt(b[5]), z1 = Integer.parseInt(b[6]);
                    for (int x = Math.min(x0, x1); x <= Math.max(x0, x1); x++)
                        for (int y = Math.min(y0, y1); y <= Math.max(y0, y1); y++)
                            for (int z = Math.min(z0, z1); z <= Math.max(z0, z1); z++)
                                level.setBlock(new BlockPos(x, y, z), state, 2 | 16);
                }
            }
        }

        String who(Entity e) {
            if (e == null) return "none";
            if (e instanceof ServerPlayer) return "player";
            return names.getOrDefault(e, "e" + e.getId());
        }

        Entity source(String word, List<Mob> mobs) {
            if (word.equals("none")) return null;
            if (word.equals("player")) return player;
            return mobs.get(Integer.parseInt(word.substring(1)));
        }

        void run(Scenario s, ServerLevel level) throws Exception {
            out.println("== " + s.name);
            this.getWorldData().overworldData().setGameTime(1000L);
            var clock = level.registryAccess().lookupOrThrow(Registries.WORLD_CLOCK).getOrThrow(WorldClocks.OVERWORLD);
            this.clockManager().setTotalTicks(clock, 6000L);
            level.setRainLevel(0.0F);
            level.setThunderLevel(0.0F);
            this.setDifficulty(s.difficulty, true);
            level.getGameRules().set(GameRules.SPAWN_WARDENS, s.wardens, this);
            names.clear();
            if (player == null) {
                CommonListenerCookie cookie = CommonListenerCookie.createInitial(new GameProfile(new UUID(0x1234L, 0x5678L), "horizon-sculk"), false);
                player = new ServerPlayer(this, level, cookie.gameProfile(), cookie.clientInformation());
                Connection connection = new Connection(PacketFlow.SERVERBOUND);
                new EmbeddedChannel(connection);
                this.getPlayerList().placeNewPlayer(connection, player, cookie);
            }
            player.removeAllEffects();
            field(ServerPlayer.class, "wardenSpawnTracker").set(player, new WardenSpawnTracker());
            if (s.warning >= 0) player.getWardenSpawnTracker().setWarningLevel(s.warning);
            double[] at = s.player != null ? s.player : new double[] {s.originX + 0.5, 100.0, s.originZ + 0.5};
            player.setGameMode(switch (s.mode) {
                case "creative" -> GameType.CREATIVE;
                case "spectator" -> GameType.SPECTATOR;
                case "adventure" -> GameType.ADVENTURE;
                default -> GameType.SURVIVAL;
            });
            player.snapTo(at[0], at[1], at[2], 0.0F, 0.0F);
            player.setDeltaMovement(Vec3.ZERO);
            player.setShiftKeyDown(s.sneak);
            List<Mob> mobs = new ArrayList<>();
            int id = 100000;
            for (MobSpec spec : s.mobs) {
                EntityType<?> type = BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(spec.type));
                Mob mob = (Mob) type.create(level, EntitySpawnReason.COMMAND);
                mob.setId(id++);
                mob.snapTo(spec.x, spec.y, spec.z, spec.yaw, 0.0F);
                mob.setYHeadRot(spec.yaw);
                mob.setYBodyRot(spec.yaw);
                for (String flag : spec.flags) {
                    if (flag.equals("persistent")) mob.setPersistenceRequired();
                    else if (flag.equals("noai")) mob.setNoAi(true);
                    else throw new IllegalArgumentException(flag);
                }
                level.addFreshEntity(mob);
                mob.getRandom().setSeed(spec.seed);
                names.put(mob, "m" + mobs.size());
                mobs.add(mob);
            }
            Object manager = get(level, "entityManager");
            Method managerTick = method(manager.getClass(), "tick");
            managerTick.invoke(manager);
            Set<Entity> known = new HashSet<>();
            Set<Entity> seeded = new HashSet<>();
            for (Entity e : allEntities(level)) known.add(e);
            if (System.getenv("SCULK_TRACE") != null) field(Level.class, "random").set(level, new TracingRandom(0L));
            level.getRandom().setSeed(s.levelSeed);
            Method tickTime = method(ServerLevel.class, "tickTime");
            Method tickBlock = method(ServerLevel.class, "tickBlock", BlockPos.class, net.minecraft.world.level.block.Block.class);
            Method tickFluid = method(ServerLevel.class, "tickFluid", BlockPos.class, net.minecraft.world.level.material.Fluid.class);
            Method runBlockEvents = method(ServerLevel.class, "runBlockEvents");
            net.minecraft.world.level.entity.EntityTickList tickList = (net.minecraft.world.level.entity.EntityTickList) get(level, "entityTickList");
            Map<String, String> last = new HashMap<>();
            String lastPlayer = "";
            String lastRng = "";
            LOG.clear();
            for (int t = 0; t <= s.ticks; t++) {
                if (t > 0) {
                    level.environmentAttributes().invalidateTickCache();
                    tickTime.invoke(level);
                    long time = level.getGameTime();
                    level.getBlockTicks().tick(time, 65536, (p, b) -> {
                        try { tickBlock.invoke(level, p, b); } catch (Exception e) { throw new RuntimeException(e); }
                    });
                    level.getFluidTicks().tick(time, 65536, (p, fl) -> {
                        try { tickFluid.invoke(level, p, fl); } catch (Exception e) { throw new RuntimeException(e); }
                    });
                    runBlockEvents.invoke(level);
                    for (Action a : s.actions) if (a.tick == t) act(level, a.words, mobs);
                    // Mobs that appeared during the block ticks (a summoned warden) tick in this
                    // tick's entity phase: seed them now, as they are seeded below.
                    List<Entity> born = new ArrayList<>();
                    for (Entity e : allEntities(level)) if (!known.contains(e)) born.add(e);
                    born.sort(Comparator.comparingInt(Entity::getId));
                    for (int i = 0; i < born.size(); i++)
                        if (born.get(i) instanceof Mob mob && seeded.add(mob)) seedFresh(mob, 0x5eedL + names.size() + i + 1);
                    // The player first (it joined first): only its warden spawn tracker.
                    player.getWardenSpawnTracker().tick();
                    tickList.forEach(e -> {
                        if (e instanceof ServerPlayer || e.isRemoved()) return;
                        e.checkDespawn();
                        if (e.isRemoved()) return;
                        level.tickNonPassenger(e);
                    });
                    level.tickBlockEntities();
                    managerTick.invoke(manager);
                }
                List<String> lines = new ArrayList<>();
                for (int[] w : s.watch) {
                    BlockPos p = new BlockPos(w[0], w[1], w[2]);
                    String line = describe(level, p);
                    String key = pos(p);
                    if (!line.equals(last.get(key))) {
                        last.put(key, line);
                        lines.add("blk " + key + " " + line);
                    }
                }
                // Regions: every block whose state changed (all of them at tick 0 are left out).
                for (int[] r : s.regions) {
                    for (int x = r[0]; x <= r[3]; x++)
                        for (int y = r[1]; y <= r[4]; y++)
                            for (int z = r[2]; z <= r[5]; z++) {
                                BlockPos p = new BlockPos(x, y, z);
                                String key = "r" + pos(p);
                                String state = BlockStateParser_serialize(level.getBlockState(p));
                                String before = last.put(key, state);
                                if (t > 0 && !state.equals(before)) lines.add("set " + pos(p) + " " + state);
                            }
                }
                WardenSpawnTracker tracker = player.getWardenSpawnTracker();
                MobEffectInstance dark = player.getEffect(MobEffects.DARKNESS);
                String now = "warn " + tracker.getWarningLevel() + " "
                    + field(WardenSpawnTracker.class, "cooldownTicks").getInt(tracker) + " dark " + (dark == null ? "-" : String.valueOf(dark.getDuration()));
                if (!now.equals(lastPlayer)) {
                    lastPlayer = now;
                    lines.add(now);
                }
                List<Entity> fresh = new ArrayList<>();
                for (Entity e : allEntities(level)) if (!known.contains(e)) fresh.add(e);
                fresh.sort(Comparator.comparingInt(Entity::getId));
                for (Entity e : fresh) {
                    known.add(e);
                    names.put(e, "n" + names.size());
                    // An entity's own random is unseeded: seed what appeared, as Horizon does.
                    if (e instanceof Mob mob && seeded.add(mob)) seedFresh(mob, 0x5eedL + names.size());
                    if (e instanceof net.minecraft.world.entity.ExperienceOrb orb) {
                        // Where an orb flies comes from its own unseeded random: its value only.
                        lines.add("new minecraft:experience_orb value=" + orb.getValue());
                        continue;
                    }
                    lines.add("new " + BuiltInRegistries.ENTITY_TYPE.getKey(e.getType()) + " " + d(e.getX()) + "," + d(e.getY()) + "," + d(e.getZ()));
                }
                lines.addAll(LOG);
                LOG.clear();
                String rng = "rng " + Long.toHexString(seedOf(level.getRandom()));
                if (!rng.equals(lastRng)) {
                    lastRng = rng;
                    lines.add(rng);
                }
                for (String line : lines) out.println(t + " " + line);
            }
            for (Mob mob : mobs) mob.discard();
            for (Entity e : allEntities(level)) if (!(e instanceof ServerPlayer)) e.discard();
            // Out of the way while the next scenario is built.
            player.snapTo(s.originX + 0.5, 100.0, s.originZ + 0.5, 0.0F, 0.0F);
            player.removeAllEffects();
            managerTick.invoke(manager);
            out.flush();
        }

        /** A scripted action: `event NAME x y z SOURCE [STATE]`, `set x y z STATE` (setBlock with update flags 3),
         *  `ignite mN` or `ate mN`. */
        void act(ServerLevel level, String[] w, List<Mob> mobs) throws Exception {
            switch (w[0]) {
                case "event" -> {
                    Holder<GameEvent> event = BuiltInRegistries.GAME_EVENT.get(Identifier.parse(w[1])).orElseThrow();
                    Vec3 at = new Vec3(Double.parseDouble(w[2]), Double.parseDouble(w[3]), Double.parseDouble(w[4]));
                    Entity source = source(w[5], mobs);
                    BlockState state = w.length > 6 ? parseState(level, w[6]) : null;
                    level.gameEvent(event, at, GameEvent.Context.of(source, state));
                }
                case "kill" -> {
                    Mob mob = mobs.get(Integer.parseInt(w[1].substring(1)));
                    mob.hurtServer(level, level.damageSources().playerAttack(player), Float.parseFloat(w.length > 2 ? w[2] : "1000"));
                }
                case "set" -> level.setBlock(new BlockPos(Integer.parseInt(w[1]), Integer.parseInt(w[2]), Integer.parseInt(w[3])), parseState(level, w[4]), 3);
                // A mob's own game events: Creeper.ignite (its next tick primes the fuse), Mob.ate.
                case "ignite" -> ((net.minecraft.world.entity.monster.Creeper) mobs.get(Integer.parseInt(w[1].substring(1)))).ignite();
                case "ate" -> mobs.get(Integer.parseInt(w[1].substring(1))).ate();
                default -> throw new IllegalArgumentException(String.join(" ", w));
            }
        }

        String describe(ServerLevel level, BlockPos p) throws Exception {
            StringBuilder sb = new StringBuilder();
            BlockState state = level.getBlockState(p);
            sb.append(BlockStateParser_serialize(state));
            BlockEntity be = level.getBlockEntity(p);
            if (be instanceof SculkSensorBlockEntity sensor) {
                sb.append(" freq=").append(sensor.getLastVibrationFrequency()).append(vibration(level, sensor.getVibrationData()));
            } else if (be instanceof SculkShriekerBlockEntity shrieker) {
                sb.append(" warn=").append(field(SculkShriekerBlockEntity.class, "warningLevel").getInt(shrieker)).append(vibration(level, shrieker.getVibrationData()));
            } else if (be instanceof SculkCatalystBlockEntity catalyst) {
                SculkSpreader spreader = catalyst.getListener().getSculkSpreader();
                sb.append(" cursors=").append(spreader.getCursors().size());
                for (SculkSpreader.ChargeCursor c : spreader.getCursors()) {
                    sb.append(' ').append(pos(c.getPos())).append(':').append(c.getCharge()).append(':')
                        .append(field(SculkSpreader.ChargeCursor.class, "updateDelay").getInt(c)).append(':').append(c.getDecayDelay()).append(':');
                    @SuppressWarnings("unchecked")
                    Set<Direction> facings = (Set<Direction>) field(SculkSpreader.ChargeCursor.class, "facings").get(c);
                    if (facings == null) sb.append('-');
                    else {
                        List<Direction> list = new ArrayList<>(facings);
                        list.sort(Comparator.comparingInt(Direction::ordinal));
                        for (Direction dir : list) sb.append(dir.getSerializedName().charAt(0));
                    }
                }
            }
            return sb.toString();
        }

        static String BlockStateParser_serialize(BlockState state) {
            return BlockStateParser.serialize(state);
        }

        String vibration(ServerLevel level, VibrationSystem.Data data) throws Exception {
            StringBuilder sb = new StringBuilder();
            VibrationInfo info = data.getCurrentVibration();
            sb.append(" cur=").append(info(level, info)).append(" travel=").append(data.getTravelTimeInTicks());
            VibrationSelector selector = data.getSelectionStrategy();
            @SuppressWarnings("unchecked")
            Optional<Object> current = (Optional<Object>) field(VibrationSelector.class, "currentVibrationData").get(selector);
            if (current.isEmpty()) sb.append(" sel=-");
            else {
                Object ev = current.get();
                VibrationInfo chosen = (VibrationInfo) method(ev.getClass(), "event").invoke(ev);
                long tick = (long) method(ev.getClass(), "tick").invoke(ev);
                sb.append(" sel=").append(info(level, chosen)).append('@').append(tick);
            }
            return sb.toString();
        }

        String info(ServerLevel level, VibrationInfo info) {
            if (info == null) return "-";
            Vec3 p = info.pos();
            Entity source = info.getEntity(level).orElse(null);
            Entity owner = info.getProjectileOwner(level).orElse(null);
            return info.gameEvent().unwrapKey().orElseThrow().identifier().getPath() + "/" + f(info.distance()) + "/" + d(p.x) + "," + d(p.y) + "," + d(p.z)
                + "/" + who(source) + "/" + who(owner);
        }

        @Override protected SampleLogger getTickTimeLogger() { return sampleLogger; }
        @Override public boolean isTickTimeLoggingEnabled() { return false; }
        @Override protected void waitUntilNextTick() { this.runAllTasks(); }
        @Override public SystemReport fillServerSystemReport(SystemReport report) { report.setDetail("Type", "Sculk oracle"); return report; }
        @Override protected void onServerExit() { super.onServerExit(); System.exit(0); }
        @Override protected void onServerCrash(net.minecraft.CrashReport report) { super.onServerCrash(report); System.exit(1); }
        @Override public boolean isHardcore() { return false; }
        @Override public LevelBasedPermissionSet operatorUserPermissions() { return LevelBasedPermissionSet.ALL; }
        @Override public PermissionSet getFunctionCompilationPermissions() { return LevelBasedPermissionSet.OWNER; }
        @Override public boolean shouldRconBroadcast() { return false; }
        @Override public boolean isDedicatedServer() { return false; }
        @Override public int getRateLimitPacketsPerSecond() { return 0; }
        @Override public int getCommandSpamThresholdSeconds() { return 0; }
        @Override public int getChatSpamThresholdSeconds() { return 0; }
        @Override public boolean useNativeTransport() { return false; }
        @Override public boolean isPublished() { return false; }
        @Override public boolean shouldInformAdmins() { return false; }
        @Override public boolean isSingleplayerOwner(NameAndId nameAndId) { return false; }
        @Override public int getMaxPlayers() { return 1; }
    }
}
