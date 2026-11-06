package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.Lifecycle;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.net.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.function.BooleanSupplier;
import net.minecraft.SharedConstants;
import net.minecraft.SystemReport;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.gametest.framework.GameTestServer;
import net.minecraft.gizmos.GizmoCollector;
import net.minecraft.gizmos.Gizmos;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.Services;
import net.minecraft.server.WorldLoader;
import net.minecraft.server.WorldStem;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.progress.LoggingLevelLoadListener;
import net.minecraft.server.notifications.EmptyNotificationService;
import net.minecraft.server.notifications.NotificationManager;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionSet;
import net.minecraft.server.players.NameAndId;
import net.minecraft.server.players.PlayerList;
import net.minecraft.server.network.Filterable;
import net.minecraft.util.Util;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.util.debugchart.LocalSampleLogger;
import net.minecraft.util.debugchart.SampleLogger;
import net.minecraft.world.Container;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.component.WritableBookContent;
import net.minecraft.world.level.DataPackConfig;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.ButtonBlock;
import net.minecraft.world.level.block.LeverBlock;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.ComparatorBlockEntity;
import net.minecraft.world.level.block.entity.LecternBlockEntity;
import net.minecraft.world.level.block.entity.TickingBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.WorldDimensions;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.storage.LevelDataAndDimensions;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.world.level.storage.PrimaryLevelData;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

/**
 * Vanilla side of tools/test_redstone_catalog_parity.py: builds each contraption of the Logical Redstone
 * Archive catalogue (block states and block entities read from its schematic by the harness) in the
 * overworld of a real flat-world server, drives its levers and buttons with useWithoutItem, and runs whole
 * ServerLevel ticks (scheduled block and fluid ticks, block events, block entities: pistons move). After each
 * tick it prints the outputs that changed; at the end of each step how long it ran, whether the level went
 * quiet, and a hash of every block in the contraption's box. `redstone_catalog_fixture` prints the same text.
 */
public final class VanillaRedstoneCatalogOracle {
    static final List<JsonObject> CASES = new ArrayList<>();
    static PrintWriter out;
    static int maxX = 16, maxZ = 16;
    /** HORIZON_CATALOG_TRACE=1: every block tick run and every block at a step's end, as `#` lines the harness ignores. */
    static final boolean TRACE = System.getenv("HORIZON_CATALOG_TRACE") != null;

    public static void main(String[] args) throws Exception {
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            if (line.isBlank()) continue;
            JsonObject c = JsonParser.parseString(line).getAsJsonObject();
            JsonArray size = c.getAsJsonArray("size");
            maxX = Math.max(maxX, size.get(0).getAsInt());
            maxZ = Math.max(maxZ, size.get(2).getAsInt());
            CASES.add(c);
        }
        out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])));
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Util.startTimerHackThread();
        Path universe = Files.createTempDirectory("horizon-redstone-catalog");
        LevelStorageSource.LevelStorageAccess access = LevelStorageSource.createDefault(universe).createAccess("catalog");
        PackRepository packs = ServerPacksSource.createPackRepository(access);
        MinecraftServer.spin(thread -> Server.create(thread, access, packs));
    }

    static Field field(Class<?> c, String name) throws Exception {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                Field f = k.getDeclaredField(name);
                f.setAccessible(true);
                return f;
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

    static BlockPos at(JsonArray a) {
        return new BlockPos(a.get(0).getAsInt(), a.get(1).getAsInt(), a.get(2).getAsInt());
    }

    static String pos(BlockPos p) {
        return p.getX() + "," + p.getY() + "," + p.getZ();
    }

    /** One block's share of the box hash; the probe computes the same (wrapping 64-bit arithmetic). */
    static long mix(int x, int y, int z, long value) {
        long k = x * 0x9E3779B97F4A7C15L + y * 0xC2B2AE3D27D4EB4FL + z * 0x165667B19E3779F9L + value * 0xD6E8FEB86659FD93L;
        k = (k ^ (k >>> 30)) * 0xBF58476D1CE4E5B9L;
        k = (k ^ (k >>> 27)) * 0x94D049BB133111EBL;
        return k ^ (k >>> 31);
    }

    static final class Server extends MinecraftServer {
        static final FeatureFlagSet FEATURES = FeatureFlags.REGISTRY
            .allFlags()
            .subtract(FeatureFlagSet.of(FeatureFlags.REDSTONE_EXPERIMENTS, FeatureFlags.MINECART_IMPROVEMENTS));
        final LocalSampleLogger sampleLogger = new LocalSampleLogger(4);
        int next = 0;
        /** Ticks the contraption may still run (the probe counts the same), and when the oracle gives up on it. */
        int left;
        long deadline;
        List<BlockPos> watched;
        List<Integer> watchedLevels;
        boolean drifted;
        boolean forced;
        BoundingBox last;

        static Server create(Thread thread, LevelStorageSource.LevelStorageAccess access, PackRepository packs) {
            try {
                packs.reload();
                ArrayList<String> enabled = new ArrayList<>(packs.getAvailableIds());
                enabled.remove("vanilla");
                enabled.addFirst("vanilla");
                WorldDataConfiguration config = new WorldDataConfiguration(new DataPackConfig(enabled, List.of()), FEATURES);
                LevelSettings settings = new LevelSettings("Catalog", GameType.CREATIVE, LevelSettings.DifficultySettings.DEFAULT, true, config);
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
                rules.set(GameRules.ADVANCE_TIME, false, null);
                rules.set(GameRules.ADVANCE_WEATHER, false, null);
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
            this.setPlayerList(new PlayerList(this, this.registries(), this.playerDataStorage, new EmptyNotificationService()) { });
            Gizmos.withCollector(GizmoCollector.NOOP);
            this.loadLevel();
            return true;
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

        /** One contraption per server tick, built and run inside it; the ticks between them update light and chunks. */
        void step(BooleanSupplier haveTime) throws Exception {
            ServerLevel level = this.overworld();
            if (!forced) {
                for (int cx = -1; cx <= (maxX >> 4) + 1; cx++)
                    for (int cz = -1; cz <= (maxZ >> 4) + 1; cz++) level.setChunkForced(cx, cz, true);
                forced = true;
            }
            super.tickServer(haveTime);
            if (!level.isPositionEntityTicking(new BlockPos(maxX + 15, 0, maxZ + 15)) || !level.isPositionEntityTicking(new BlockPos(-15, 0, -15))) return;
            if (next >= CASES.size()) {
                out.close();
                Runtime.getRuntime().halt(0);
                return;
            }
            JsonObject c = CASES.get(next++);
            out.println("case " + c.get("code").getAsString());
            long started = System.nanoTime();
            try {
                run(c, level);
            } catch (Exception e) {
                out.println("error " + e);
            }
            out.println("# ms " + (System.nanoTime() - started) / 1_000_000);
            out.flush();
        }

        void clear(ServerLevel level) throws Exception {
            if (last == null) return;
            BlockState air = Blocks.AIR.defaultBlockState();
            for (BlockPos p : BlockPos.betweenClosed(last.minX(), last.minY(), last.minZ(), last.maxX(), last.maxY(), last.maxZ()))
                if (!level.getBlockState(p).isAir()) level.setBlock(p, air, 2 | 16 | 32 | 256 | 512);
            // Ticks anywhere in the forced chunks (the quiet check counts them all) and dropped items go too.
            BoundingBox all = new BoundingBox(-16, level.getMinY(), -16, maxX + 31, level.getMaxY(), maxZ + 31);
            level.getBlockTicks().clearArea(all);
            level.getFluidTicks().clearArea(all);
            List<net.minecraft.world.entity.Entity> entities = new ArrayList<>();
            for (net.minecraft.world.entity.Entity e : level.getAllEntities()) if (e != null) entities.add(e);
            entities.forEach(net.minecraft.world.entity.Entity::discard);
            ((java.util.Collection<?>) field(ServerLevel.class, "blockEvents").get(level)).clear();
        }

        void run(JsonObject c, ServerLevel level) throws Exception {
            clear(level);
            JsonArray size = c.getAsJsonArray("size");
            int w = size.get(0).getAsInt(), h = size.get(1).getAsInt(), l = size.get(2).getAsInt();
            // Pistons can push a block one cell past the schematic: clear and hash a margin.
            BoundingBox box = new BoundingBox(-2, -2, -2, w + 1, h + 1, l + 1);
            last = box;
            // The clock keeps running (LevelTicks keys its chunks by trigger time): the probe starts at this time.
            out.println("time " + level.getGameTime());
            for (var e : c.getAsJsonArray("cells")) {
                JsonArray a = e.getAsJsonArray();
                level.setBlock(new BlockPos(a.get(0).getAsInt(), a.get(1).getAsInt(), a.get(2).getAsInt()), Block.stateById(a.get(3).getAsInt()), 2 | 16 | 512);
            }
            for (var e : c.getAsJsonArray("comparators")) {
                JsonArray a = e.getAsJsonArray();
                if (level.getBlockEntity(at(a)) instanceof ComparatorBlockEntity be) be.setOutputSignal(a.get(3).getAsInt());
            }
            for (var e : c.getAsJsonArray("containers")) {
                JsonArray a = e.getAsJsonArray();
                if (!(level.getBlockEntity(at(a)) instanceof Container container)) continue;
                for (var s : a.get(3).getAsJsonArray()) {
                    JsonArray slot = s.getAsJsonArray();
                    var item = BuiltInRegistries.ITEM.getOptional(Identifier.parse(slot.get(1).getAsString())).orElse(Items.AIR);
                    int index = slot.get(0).getAsInt();
                    if (index >= 0 && index < container.getContainerSize()) container.setItem(index, new ItemStack(item, slot.get(2).getAsInt()));
                }
            }
            for (var e : c.getAsJsonArray("lecterns")) {
                JsonArray a = e.getAsJsonArray();
                if (!(level.getBlockEntity(at(a)) instanceof LecternBlockEntity lectern)) continue;
                ItemStack book = new ItemStack(Items.WRITABLE_BOOK);
                List<Filterable<String>> pages = new ArrayList<>();
                for (int i = 0; i < a.get(3).getAsInt(); i++) pages.add(Filterable.passThrough(""));
                book.set(DataComponents.WRITABLE_BOOK_CONTENT, new WritableBookContent(pages));
                lectern.setBook(book);
                // The saved page, set silently: setPage would pulse the lectern (signalPageChange) as a player's turn does.
                int page = Math.max(0, Math.min(a.get(4).getAsInt(), a.get(3).getAsInt() - 1));
                field(LecternBlockEntity.class, "page").setInt(lectern, page);
            }
            // Filling a container or lectern updated its comparators (BlockEntity.setChanged), which scheduled
            // ticks before the wake below; the probe sets its levels silently. Start both from no ticks.
            BoundingBox all = new BoundingBox(-16, level.getMinY(), -16, maxX + 31, level.getMaxY(), maxZ + 31);
            level.getBlockTicks().clearArea(all);
            // The comparator level of every block entity that has one: the probe reads these.
            // Those next to something that moves items are watched: the probe keeps every level fixed.
            watched = new ArrayList<>();
            watchedLevels = new ArrayList<>();
            drifted = false;
            for (BlockPos p : BlockPos.betweenClosed(0, 0, 0, w - 1, h - 1, l - 1)) {
                BlockState s = level.getBlockState(p);
                if (s.hasBlockEntity() && s.hasAnalogOutputSignal()) {
                    int v = s.getAnalogOutputSignal(level, p, Direction.NORTH);
                    out.println("analog " + pos(p) + " " + v);
                    boolean near = movesItems(s);
                    for (Direction d : Direction.values()) near |= movesItems(level.getBlockState(p.relative(d)));
                    if (near) {
                        watched.add(p.immutable());
                        watchedLevels.add(v);
                    }
                }
            }
            List<BlockPos> outputs = new ArrayList<>();
            for (var e : c.getAsJsonArray("outputs")) outputs.add(at(e.getAsJsonArray()));
            // Wake: every block hears that its neighbours may have changed, in schematic order.
            for (var e : c.getAsJsonArray("cells")) {
                JsonArray a = e.getAsJsonArray();
                BlockPos p = new BlockPos(a.get(0).getAsInt(), a.get(1).getAsInt(), a.get(2).getAsInt());
                BlockState s = level.getBlockState(p);
                s.handleNeighborChanged(level, p, s.getBlock(), null, false);
            }
            out.println("step init");
            left = c.get("budget").getAsInt();
            deadline = System.nanoTime() + 240_000_000_000L;
            ticks(level, outputs, box, c.get("settle").getAsInt());
            int limit = c.get("max").getAsInt();
            int index = 0;
            for (var e : c.getAsJsonArray("steps")) {
                if (left <= 0) {
                    out.println("budget spent");
                    break;
                }
                if (System.nanoTime() > deadline) {
                    out.println("error timeout before step " + index);
                    break;
                }
                out.println("step " + index++);
                for (var act : e.getAsJsonArray()) {
                    JsonArray a = act.getAsJsonArray();
                    BlockPos p = at(a);
                    BlockState s = level.getBlockState(p);
                    boolean on = a.get(3).getAsInt() != 0;
                    boolean use = s.getBlock() instanceof LeverBlock ? s.getValue(BlockStateProperties.POWERED) != on
                        : s.getBlock() instanceof ButtonBlock && on;
                    if (use) s.useWithoutItem(level, null, new BlockHitResult(Vec3.atCenterOf(p), Direction.UP, p, false));
                }
                ticks(level, outputs, box, limit);
            }
        }

        static boolean movesItems(BlockState s) {
            return s.is(Blocks.HOPPER) || s.is(Blocks.DROPPER) || s.is(Blocks.DISPENSER) || s.is(Blocks.CRAFTER);
        }

        /** Prints the first watched comparator level that changed: the run can't be compared after it. */
        void checkDrift(ServerLevel level, int t) {
            for (int i = 0; i < watched.size() && !drifted; i++) {
                BlockState s = level.getBlockState(watched.get(i));
                int v = s.hasAnalogOutputSignal() ? s.getAnalogOutputSignal(level, watched.get(i), Direction.NORTH) : -1;
                if (v != watchedLevels.get(i)) {
                    out.println("drift " + pos(watched.get(i)) + " " + watchedLevels.get(i) + " " + v + " at tick " + t);
                    drifted = true;
                }
            }
        }

        static String outputs(ServerLevel level, List<BlockPos> outputs) {
            StringBuilder sb = new StringBuilder();
            for (BlockPos p : outputs) {
                if (sb.length() > 0) sb.append(',');
                sb.append(Block.getId(level.getBlockState(p)));
                if (level.getBlockEntity(p) instanceof ComparatorBlockEntity be) sb.append('/').append(be.getOutputSignal());
            }
            return sb.toString();
        }

        /** Whole level ticks until nothing is scheduled, queued or moving, or `limit` ticks. */
        void ticks(ServerLevel level, List<BlockPos> outputs, BoundingBox box, int limit) throws Exception {
            Method tickTime = method(ServerLevel.class, "tickTime");
            Method tickBlock = method(ServerLevel.class, "tickBlock", BlockPos.class, Block.class);
            Method tickFluid = method(ServerLevel.class, "tickFluid", BlockPos.class, net.minecraft.world.level.material.Fluid.class);
            Method runBlockEvents = method(ServerLevel.class, "runBlockEvents");
            String last = outputs(level, outputs);
            out.println("t 0 " + last);
            int t = 0;
            while (t < limit && left > 0 && !quiet(level)) {
                t++;
                left--;
                tickTime.invoke(level);
                long time = level.getGameTime();
                level.getBlockTicks().tick(time, 65536, (p, b) -> {
                    if (TRACE) out.println("# tick " + pos(p) + " " + BuiltInRegistries.BLOCK.getKey(b) + " " + level.getBlockState(p));
                    try { tickBlock.invoke(level, p, b); } catch (Exception e) { throw new RuntimeException(e); }
                });
                level.getFluidTicks().tick(time, 65536, (p, f) -> {
                    try { tickFluid.invoke(level, p, f); } catch (Exception e) { throw new RuntimeException(e); }
                });
                runBlockEvents.invoke(level);
                level.tickBlockEntities();
                checkDrift(level, t);
                String now = outputs(level, outputs);
                if (!now.equals(last)) out.println("t " + t + " " + now);
                last = now;
            }
            long hash = 0;
            for (BlockPos p : BlockPos.betweenClosed(box.minX(), box.minY(), box.minZ(), box.maxX(), box.maxY(), box.maxZ())) {
                BlockState s = level.getBlockState(p);
                if (s.isAir()) continue;
                long value = Block.getId(s);
                if (level.getBlockEntity(p) instanceof ComparatorBlockEntity be) value += (long) (be.getOutputSignal() + 1) << 32;
                hash += mix(p.getX(), p.getY(), p.getZ(), value);
                if (TRACE) out.println("# cell " + pos(p) + " " + value);
            }
            out.println("end " + t + " " + (quiet(level) ? 1 : 0) + " " + Long.toHexString(hash));
        }

        @SuppressWarnings("unchecked")
        static boolean quiet(ServerLevel level) throws Exception {
            if (level.getBlockTicks().count() > 0 || level.getFluidTicks().count() > 0) return false;
            if (!((java.util.Collection<?>) field(ServerLevel.class, "blockEvents").get(level)).isEmpty()) return false;
            for (String name : new String[] {"blockEntityTickers", "pendingBlockEntityTickers"})
                for (TickingBlockEntity t : (List<TickingBlockEntity>) field(Level.class, name).get(level))
                    if (!t.isRemoved() && level.getBlockState(t.getPos()).is(Blocks.MOVING_PISTON)) return false;
            return true;
        }

        @Override protected SampleLogger getTickTimeLogger() { return sampleLogger; }
        @Override public boolean isTickTimeLoggingEnabled() { return false; }
        @Override protected void waitUntilNextTick() { this.runAllTasks(); }
        @Override public SystemReport fillServerSystemReport(SystemReport report) { report.setDetail("Type", "Redstone catalog oracle"); return report; }
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
