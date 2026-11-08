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

    static JsonObject raidActor(net.minecraft.world.entity.LivingEntity entity, ServerLevel level) {
        JsonObject raw=new JsonObject();UUID u=entity.getUUID();
        JsonArray words=new JsonArray();for (int w:new int[]{(int)(u.getMostSignificantBits()>>32),(int)u.getMostSignificantBits(),(int)(u.getLeastSignificantBits()>>32),(int)u.getLeastSignificantBits()}) words.add(w);raw.add("uuid",words);
        raw.addProperty("type",net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()).toString());raw.addProperty("health",entity.getHealth());
        JsonArray pos=new JsonArray();pos.add(entity.getX());pos.add(entity.getY());pos.add(entity.getZ());raw.add("pos",pos);
        raw.addProperty("noai",entity instanceof net.minecraft.world.entity.Mob mob && mob.isNoAi());
        raw.addProperty("raid_id",entity instanceof net.minecraft.world.entity.raid.Raider r && r.getCurrentRaid()!=null ? level.getRaids().getId(r.getCurrentRaid()).orElse(-1):-1);
        JsonArray home=new JsonArray();if (entity instanceof net.minecraft.world.entity.npc.villager.Villager v) v.getBrain().getMemory(net.minecraft.world.entity.ai.memory.MemoryModuleType.HOME).ifPresent(p->{home.add(p.pos().getX());home.add(p.pos().getY());home.add(p.pos().getZ());});raw.add("home",home);
        var output=net.minecraft.world.level.storage.TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,level.registryAccess());entity.saveWithoutId(output);raw.addProperty("nbt",output.buildResult().toString());return raw;
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
            WorldLoader.InitConfig init = new WorldLoader.InitConfig(packConfig, Commands.CommandSelection.INTEGRATED,
                Boolean.getBoolean("horizon.functionAdmissionFixture") ? LevelBasedPermissionSet.GAMEMASTER : LevelBasedPermissionSet.OWNER);
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

        // The fallback table has no random_sequence. Use the actual rod item,
        // so the level-random pitch draw happens at vanilla's real call site.
        void writeFishingFallbackReference() throws Exception {
            if (!Boolean.getBoolean("horizon.fishingFallback")) return;
            var level = this.overworld();
            var nibble = net.minecraft.world.entity.projectile.FishingHook.class.getDeclaredField("nibble");
            nibble.setAccessible(true);
            JsonArray rows = new JsonArray();
            for (long seed : new long[] {0, 1, 42, 123456789, -1}) {
                player.setItemInHand(net.minecraft.world.InteractionHand.MAIN_HAND,
                    new net.minecraft.world.item.ItemStack(net.minecraft.world.item.Items.FISHING_ROD));
                var hook = new net.minecraft.world.entity.projectile.FishingHook(player, level, 0, 0);
                hook.getRandom().setSeed(seed);
                nibble.setInt(hook, 10);
                var area = player.getBoundingBox().inflate(12.0);
                var before = new java.util.HashSet<java.util.UUID>();
                for (var entity : level.getEntities((net.minecraft.world.entity.Entity)null, area)) before.add(entity.getUUID());
                level.getRandom().setSeed(seed);
                net.minecraft.world.item.Items.FISHING_ROD.use(level, player, net.minecraft.world.InteractionHand.MAIN_HAND);
                JsonArray items = new JsonArray();
                JsonArray xp = new JsonArray();
                for (var entity : level.getEntities((net.minecraft.world.entity.Entity)null, area)) {
                    if (before.contains(entity.getUUID())) continue;
                    if (entity instanceof net.minecraft.world.entity.item.ItemEntity item) {
                        items.add(net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item.getItem().getItem()).toString());
                        item.discard();
                    } else if (entity instanceof net.minecraft.world.entity.ExperienceOrb orb) {
                        xp.add(orb.getValue());
                        orb.discard();
                    }
                }
                JsonObject row = new JsonObject();
                row.addProperty("seed", seed);
                row.addProperty("level_next", level.getRandom().nextLong());
                row.add("items", items);
                row.add("xp", xp);
                rows.add(row);
            }
            Files.writeString(output.resolveSibling("fishing-fallback-reference.json"), rows.toString());
        }

        void writeHappyGhastLeashReference() throws Exception {
            if (!Boolean.getBoolean("horizon.happyGhastLeashReference")) return;
            var level = this.overworld();
            var fencePos = new net.minecraft.core.BlockPos(0, 64, 0);
            var oldBlock = level.getBlockState(fencePos);
            level.setBlockAndUpdate(fencePos, net.minecraft.world.level.block.Blocks.OAK_FENCE.defaultBlockState());
            JsonArray rows = new JsonArray();
            for (String kind : new String[] {"adult", "baby", "cow"}) {
                for (int probe = 0; probe < 12; probe++) {
                    double slack = kind.equals("cow") ? 6.0 : 10.0;
                    double snap = kind.equals("cow") ? 12.0 : 16.0;
                    double distance = probe < 6 ? new int[] {2, 4, 8, 11, 13, 17}[probe]
                        : (probe < 9 ? slack : snap) + ((probe - 6) % 3 - 1) * 1.0e-6;
                    String phase = probe < 6 ? "dispatch" : probe < 9 ? "elastic" : "snap";
                    net.minecraft.world.entity.PathfinderMob mob;
                    if (kind.equals("cow")) {
                        mob = new net.minecraft.world.entity.animal.cow.Cow(net.minecraft.world.entity.EntityTypes.COW, level);
                    } else {
                        var ghast = new net.minecraft.world.entity.animal.happyghast.HappyGhast(net.minecraft.world.entity.EntityTypes.HAPPY_GHAST, level);
                        ghast.setAge(kind.equals("baby") ? -24000 : 0);
                        mob = ghast;
                    }
                    var knot = new net.minecraft.world.entity.decoration.LeashFenceKnotEntity(level, fencePos);
                    if (phase.equals("elastic")) {
                        mob.setPos(knot.getX() + distance, knot.getY() + knot.getBbHeight() / 2.0 - mob.getBbHeight() / 2.0,
                            knot.getZ() - mob.getBbWidth() / 2.0);
                    } else if (phase.equals("snap")) {
                        var center = knot.getBoundingBox().getCenter();
                        mob.setPos(center.x + distance, center.y - mob.getBbHeight() / 2.0, center.z);
                    } else mob.setPos(0.5 + distance, 64, 0.5);
                    mob.setYRot(0);
                    mob.setOnGround(false);
                    mob.setDeltaMovement(net.minecraft.world.phys.Vec3.ZERO);
                    var marker = new net.minecraft.core.BlockPos(8, 64, 8);
                    var path = new net.minecraft.world.level.pathfinder.Path(
                        List.of(new net.minecraft.world.level.pathfinder.Node(8, 64, 8)), marker, true);
                    if (!mob.getNavigation().moveTo(path, 0.75)) throw new IllegalStateException("marker path rejected");
                    mob.getMoveControl().setWantedPosition(20, 70, 20, 1.0);
                    mob.setLeashedTo(knot, false);
                    net.minecraft.world.entity.Leashable.tickLeash(level, mob);
                    JsonObject row = new JsonObject();
                    row.addProperty("kind", kind);
                    row.addProperty("distance", distance);
                    row.addProperty("phase", phase);
                    JsonArray position = new JsonArray();
                    position.add(mob.getX()); position.add(mob.getY()); position.add(mob.getZ());
                    row.add("position", position);
                    row.addProperty("snapped", !mob.isLeashed());
                    row.addProperty("wanted", mob.getMoveControl().hasWanted());
                    var currentPath = mob.getNavigation().getPath();
                    if (currentPath == null) row.add("path", com.google.gson.JsonNull.INSTANCE);
                    else {
                        JsonArray target = new JsonArray();
                        target.add(currentPath.getTarget().getX());
                        target.add(currentPath.getTarget().getY());
                        target.add(currentPath.getTarget().getZ());
                        row.add("path", target);
                    }
                    JsonArray velocity = new JsonArray();
                    var v = mob.getDeltaMovement();
                    velocity.add(v.x); velocity.add(v.y); velocity.add(v.z);
                    row.add("velocity", velocity);
                    rows.add(row);
                    mob.removeLeash();
                    mob.discard();
                    knot.discard();
                }
            }
            level.setBlockAndUpdate(fencePos, oldBlock);
            Files.writeString(output.resolveSibling("happy-ghast-leash-reference.json"), rows.toString());
        }

        void writeMountControlReference() throws Exception {
            if (!Boolean.getBoolean("horizon.mountControlReference")) return;
            var level = this.overworld();
            var oldMain = player.getMainHandItem();
            var oldOff = player.getOffhandItem();
            var oldPosition = player.position();
            JsonArray rows = new JsonArray();
            for (String kind : new String[] {"pig", "strider"}) {
                for (boolean saddled : new boolean[] {false, true}) {
                    for (boolean first : new boolean[] {false, true}) {
                        for (String hand : new String[] {"none", "wrong", "main", "off"}) {
                            net.minecraft.world.entity.Mob mount = kind.equals("pig")
                                ? new net.minecraft.world.entity.animal.pig.Pig(net.minecraft.world.entity.EntityTypes.PIG, level)
                                : new net.minecraft.world.entity.monster.Strider(net.minecraft.world.entity.EntityTypes.STRIDER, level);
                            mount.setPos(oldPosition);
                            mount.setItemSlot(net.minecraft.world.entity.EquipmentSlot.SADDLE,
                                saddled ? new net.minecraft.world.item.ItemStack(net.minecraft.world.item.Items.SADDLE)
                                    : net.minecraft.world.item.ItemStack.EMPTY);
                            var stick = kind.equals("pig") ? net.minecraft.world.item.Items.CARROT_ON_A_STICK
                                : net.minecraft.world.item.Items.WARPED_FUNGUS_ON_A_STICK;
                            var wrong = kind.equals("pig") ? net.minecraft.world.item.Items.WARPED_FUNGUS_ON_A_STICK
                                : net.minecraft.world.item.Items.CARROT_ON_A_STICK;
                            player.setItemInHand(net.minecraft.world.InteractionHand.MAIN_HAND,
                                hand.equals("main") ? new net.minecraft.world.item.ItemStack(stick)
                                    : hand.equals("wrong") ? new net.minecraft.world.item.ItemStack(wrong)
                                    : net.minecraft.world.item.ItemStack.EMPTY);
                            player.setItemInHand(net.minecraft.world.InteractionHand.OFF_HAND,
                                hand.equals("off") ? new net.minecraft.world.item.ItemStack(stick)
                                    : net.minecraft.world.item.ItemStack.EMPTY);
                            var other = new net.minecraft.world.entity.player.Player(level,
                                new GameProfile(new UUID(0, 2), "OtherRider")) {
                                public net.minecraft.world.level.GameType gameMode() {
                                    return net.minecraft.world.level.GameType.SURVIVAL;
                                }
                            };
                            if (!first && !other.startRiding(mount, true, true)) throw new IllegalStateException("first passenger rejected");
                            if (!player.startRiding(mount, true, true)) throw new IllegalStateException("player passenger rejected");
                            if ((mount.getFirstPassenger() == player) != first) throw new IllegalStateException("passenger order differs");
                            var input = mount.getClass().getDeclaredMethod("getRiddenInput", net.minecraft.world.entity.player.Player.class,
                                net.minecraft.world.phys.Vec3.class);
                            input.setAccessible(true);
                            var direction = (net.minecraft.world.phys.Vec3)input.invoke(mount, player, net.minecraft.world.phys.Vec3.ZERO);
                            JsonObject row = new JsonObject();
                            row.addProperty("kind", kind);
                            row.addProperty("saddled", saddled);
                            row.addProperty("first", first);
                            row.addProperty("hand", hand);
                            row.addProperty("controlled", mount.getControllingPassenger() == player);
                            row.addProperty("server_simulates", mount.canSimulateMovement());
                            row.addProperty("forward", direction.z);
                            rows.add(row);
                            player.stopRiding();
                            other.stopRiding();
                            other.discard();
                            mount.discard();
                        }
                    }
                }
            }
            player.setItemInHand(net.minecraft.world.InteractionHand.MAIN_HAND, oldMain);
            player.setItemInHand(net.minecraft.world.InteractionHand.OFF_HAND, oldOff);
            player.setPos(oldPosition);
            Files.writeString(output.resolveSibling("mount-control-reference.json"), rows.toString());
        }

        // Direct reference for FishingHook.retrieve on a real server player and level.
        void writeFishingReference() throws Exception {
            if (!Boolean.getBoolean("horizon.fishingReference")) return;
            var level = this.overworld();
            var sequence = net.minecraft.resources.Identifier.parse("minecraft:gameplay/fishing");
            var nibble = net.minecraft.world.entity.projectile.FishingHook.class.getDeclaredField("nibble");
            nibble.setAccessible(true);
            JsonArray rows = new JsonArray();
            for (long seed : new long[] {0, 1, 42, 123456789, -1}) {
                this.getRandomSequences().reset(sequence, seed);
                player.setItemInHand(net.minecraft.world.InteractionHand.MAIN_HAND,
                    new net.minecraft.world.item.ItemStack(net.minecraft.world.item.Items.FISHING_ROD));
                var hook = new net.minecraft.world.entity.projectile.FishingHook(player, level, 0, 0);
                hook.getRandom().setSeed(seed);
                nibble.setInt(hook, 10);
                var area = player.getBoundingBox().inflate(12.0);
                var before = new java.util.HashSet<java.util.UUID>();
                for (var entity : level.getEntities((net.minecraft.world.entity.Entity)null, area)) before.add(entity.getUUID());
                int damage = hook.retrieve(player.getMainHandItem());
                JsonArray drops = new JsonArray();
                for (var entity : level.getEntities((net.minecraft.world.entity.Entity)null, area)) {
                    if (before.contains(entity.getUUID())) continue;
                    if (entity instanceof net.minecraft.world.entity.item.ItemEntity item) {
                        JsonObject drop = new JsonObject();
                        drop.addProperty("item", net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item.getItem().getItem()).toString());
                        drop.addProperty("count", item.getItem().getCount());
                        drops.add(drop);
                        item.discard();
                    } else if (entity instanceof net.minecraft.world.entity.ExperienceOrb orb) {
                        JsonObject drop = new JsonObject();
                        drop.addProperty("xp", orb.getValue());
                        drops.add(drop);
                        orb.discard();
                    }
                }
                JsonObject row = new JsonObject();
                row.addProperty("hook_seed", seed);
                row.addProperty("sequence_seed", seed);
                row.addProperty("damage", damage);
                row.addProperty("hook_next", hook.getRandom().nextLong());
                row.addProperty("sequence_next", this.getRandomSequence(sequence).nextLong());
                row.add("drops", drops);
                rows.add(row);
            }
            Files.writeString(output.resolveSibling("fishing-reference.json"), rows.toString());

            // Invoke the actual private catchingFish on the real level, above
            // build height in clear weather: splash checks see air and sky.
            this.setWeatherParameters(100000, 0, false, false);
            var hookClass = net.minecraft.world.entity.projectile.FishingHook.class;
            var lured = hookClass.getDeclaredField("timeUntilLured");
            var hooked = hookClass.getDeclaredField("timeUntilHooked");
            var angle = hookClass.getDeclaredField("fishAngle");
            for (var f : new java.lang.reflect.Field[] {lured, hooked, angle}) f.setAccessible(true);
            var catching = hookClass.getDeclaredMethod("catchingFish", net.minecraft.core.BlockPos.class);
            catching.setAccessible(true);
            JsonArray ticks = new JsonArray();
            for (long seed : new long[] {0, 1, 42, 123456789, -1}) {
                for (int phase = 0; phase < 5; phase++) {
                    var hook = new net.minecraft.world.entity.projectile.FishingHook(player, level, 0, 0);
                    hook.setPos(0.5, 400, 0.5);
                    hook.getRandom().setSeed(seed);
                    nibble.setInt(hook, phase == 4 ? 2 : 0);
                    lured.setInt(hook, phase == 1 ? 2 : 0);
                    hooked.setInt(hook, phase == 2 ? 3 : phase == 3 ? 1 : 0);
                    angle.setFloat(hook, 12.5F);
                    catching.invoke(hook, new net.minecraft.core.BlockPos(0, 400, 0));
                    JsonObject row = new JsonObject();
                    row.addProperty("seed", seed);
                    row.addProperty("phase", phase);
                    row.addProperty("nibble", nibble.getInt(hook));
                    row.addProperty("lured", lured.getInt(hook));
                    row.addProperty("hooked", hooked.getInt(hook));
                    row.addProperty("angle_bits", Float.floatToRawIntBits(angle.getFloat(hook)));
                    row.addProperty("next", hook.getRandom().nextLong());
                    ticks.add(row);
                    hook.discard();
                }
            }
            Files.writeString(output.resolveSibling("fishing-ticks-reference.json"), ticks.toString());

            // Undo only the constructor's twelve triangle-related LCG steps.
            // Entity's earlier UUID draws remain reflected in the recovered seed.
            var seedField = net.minecraft.world.level.levelgen.LegacyRandomSource.class.getDeclaredField("seed");
            seedField.setAccessible(true);
            long multiplier = 0x5deece66dL;
            long mask = (1L << 48) - 1;
            long inverse = java.math.BigInteger.valueOf(multiplier)
                .modInverse(java.math.BigInteger.ONE.shiftLeft(48)).longValue();
            JsonArray casts = new JsonArray();
            float originalYaw = player.getYRot(), originalPitch = player.getXRot();
            for (float[] orientation : new float[][] {{0, 0}, {45, 30}, {-135, -60}, {180, 89}, {12, -89}}) {
                player.setYRot(orientation[0]);
                player.setXRot(orientation[1]);
                var hook = new net.minecraft.world.entity.projectile.FishingHook(player, level, 0, 0);
                long state = ((java.util.concurrent.atomic.AtomicLong) seedField.get(hook.getRandom())).get();
                for (int i = 0; i < 12; i++) state = ((state - 11) * inverse) & mask;
                var velocity = hook.getDeltaMovement();
                JsonObject row = new JsonObject();
                row.addProperty("yaw", orientation[0]);
                row.addProperty("pitch", orientation[1]);
                row.addProperty("seed", state ^ multiplier);
                row.addProperty("x_bits", Double.doubleToRawLongBits(velocity.x));
                row.addProperty("y_bits", Double.doubleToRawLongBits(velocity.y));
                row.addProperty("z_bits", Double.doubleToRawLongBits(velocity.z));
                row.addProperty("next", hook.getRandom().nextLong());
                casts.add(row);
                hook.discard();
            }
            player.setYRot(originalYaw);
            player.setXRot(originalPitch);
            Files.writeString(output.resolveSibling("fishing-casts-reference.json"), casts.toString());

            // Actual tick in a single still-water block, before a bite.
            var waterPos = new net.minecraft.core.BlockPos(0, 300, 0);
            var oldBlock = level.getBlockState(waterPos);
            var oldPos = player.position();
            level.setBlockAndUpdate(waterPos, net.minecraft.world.level.block.Blocks.WATER.defaultBlockState());
            player.snapTo(0.5, 300.0, -1.5, 0, 0);
            var stateField = hookClass.getDeclaredField("currentState");
            stateField.setAccessible(true);
            Object bobbing = java.util.Arrays.stream(stateField.getType().getEnumConstants())
                .filter(v -> v.toString().equals("BOBBING")).findFirst().orElseThrow();
            JsonArray bobs = new JsonArray();
            for (long seed : new long[] {0, 1, 42, 123456789, -1}) {
                player.setItemInHand(net.minecraft.world.InteractionHand.MAIN_HAND,
                    new net.minecraft.world.item.ItemStack(net.minecraft.world.item.Items.FISHING_ROD));
                var hook = new net.minecraft.world.entity.projectile.FishingHook(player, level, 0, 0);
                hook.setPos(0.5, 300.5, 0.5);
                hook.setDeltaMovement(0.1, 0.03, -0.2);
                hook.getRandom().setSeed(seed);
                stateField.set(hook, bobbing);
                nibble.setInt(hook, 0);
                hook.tick();
                var v = hook.getDeltaMovement();
                JsonObject row = new JsonObject();
                row.addProperty("seed", seed);
                row.addProperty("vx", Double.doubleToRawLongBits(v.x));
                row.addProperty("vy", Double.doubleToRawLongBits(v.y));
                row.addProperty("vz", Double.doubleToRawLongBits(v.z));
                row.addProperty("nibble", nibble.getInt(hook));
                row.addProperty("next", hook.getRandom().nextLong());
                bobs.add(row);
                hook.discard();
            }
            // Dry block hits run on both sides: compare the client's movement
            // prediction with the real hook before and after touching the floor.
            level.setBlockAndUpdate(waterPos, net.minecraft.world.level.block.Blocks.STONE.defaultBlockState());
            double[][] motions = {{0.2, -0.4, 0.1}, {-0.2, -0.4, 0.1}, {0.0, -0.6, -0.2}};
            for (int c = 0; c < motions.length; c++) {
                var hook = new net.minecraft.world.entity.projectile.FishingHook(player, level, 0, 0);
                hook.setPos(0.5, 301.1, 0.5);
                hook.setDeltaMovement(motions[c][0], motions[c][1], motions[c][2]);
                for (int tick = 0; tick < 5; tick++) {
                    hook.tick();
                    var at = hook.position();
                    var motion = hook.getDeltaMovement();
                    JsonObject row = new JsonObject();
                    row.addProperty("case", c);
                    row.addProperty("tick", tick);
                    JsonArray position = new JsonArray();
                    position.add(at.x); position.add(at.y); position.add(at.z);
                    JsonArray movement = new JsonArray();
                    movement.add(motion.x); movement.add(motion.y); movement.add(motion.z);
                    row.add("position", position);
                    row.add("motion", movement);
                    bobs.add(row);
                }
                hook.discard();
            }
            player.snapTo(oldPos.x, oldPos.y, oldPos.z, originalYaw, originalPitch);
            level.setBlockAndUpdate(waterPos, oldBlock);
            Files.writeString(output.resolveSibling("fishing-bobbing-reference.json"), bobs.toString());
        }

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
            if(Boolean.getBoolean("horizon.merchantFixture")||Boolean.getBoolean("horizon.merchantCraftedFixture")) this.tickRateManager().setFrozen(true);
            Connection connection = new Connection(PacketFlow.SERVERBOUND);
            EmbeddedChannel channel = Boolean.getBoolean("horizon.merchantCraftedFixture") ? new EmbeddedChannel(
                new io.netty.channel.ChannelInitializer<io.netty.channel.Channel>() {
                    @Override protected void initChannel(io.netty.channel.Channel ch) {
                        Connection.configureSerialization(ch.pipeline(),PacketFlow.SERVERBOUND,false,null);
                        ch.pipeline().addLast("packet_handler",connection);
                    }
                }) : new EmbeddedChannel(connection);
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
            if(Boolean.getBoolean("horizon.merchantCraftedFixture")) connection.setupOutboundProtocol(
                net.minecraft.network.protocol.game.GameProtocols.CLIENTBOUND_TEMPLATE.bind(
                    net.minecraft.network.RegistryFriendlyByteBuf.decorator(this.registryAccess())));
            this.getPlayerList().placeNewPlayer(connection, player, cookie);
            if(Boolean.getBoolean("horizon.merchantCraftedFixture")) {channel.runPendingTasks();channel.flushOutbound();channel.checkException();if(!channel.isActive())throw new IllegalStateException("merchant channel inactive");}
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
            try { writeHappyGhastLeashReference(); } catch (Exception e) { throw new IOException(e); }
            try { writeMountControlReference(); } catch (Exception e) { throw new IOException(e); }
            try { writeFishingReference(); } catch (Exception e) { throw new IOException(e); }
            try { writeFishingFallbackReference(); } catch (Exception e) { throw new IOException(e); }
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
                        // Reuse the existing feedback receiver while retaining the actual
                        // server stack's null entity, names, position and permissions.
                        var baseSource = op.has("source") && op.get("source").getAsString().equals("console")
                                ? this.createCommandSourceStack().withSource(player.commandSource())
                                : player.createCommandSourceStack();
                        var source = baseSource.withCallback((success, value) -> {
                            JsonArray r = new JsonArray();
                            r.add(success);
                            r.add(value);
                            results.add(r);
                        });
                        this.getCommands().performPrefixedCommand(source, command);
                        if (!op.has("record") || op.get("record").getAsBoolean() || (op.has("trace") && op.get("trace").getAsBoolean())) {
                            JsonObject record = new JsonObject();
                            record.addProperty("cmd", command);
                            JsonArray m = new JsonArray();
                            messages.forEach(m::add);
                            record.add("messages", m);
                            JsonArray r = new JsonArray();
                            results.forEach(r::add);
                            record.add("results", r);
                            if (op.has("trace") && op.get("trace").getAsBoolean()) System.err.println("command trace: " + gson.toJson(record));
                            if (!op.has("record") || op.get("record").getAsBoolean()) out.append(gson.toJson(record)).append('\n');
                        }
                    }
                    case "projectile_saved_type" -> {
                        var entity=level.getEntity(UUID.fromString(op.get("uuid").getAsString()));
                        if (!(entity instanceof net.minecraft.world.entity.projectile.arrow.AbstractArrow) || !entity.isAlive())
                            throw new IllegalStateException("projectile type: missing actual arrow");
                        var uuid=entity.getUUID();
                        int[] words={(int)(uuid.getMostSignificantBits()>>32),(int)uuid.getMostSignificantBits(),
                            (int)(uuid.getLeastSignificantBits()>>32),(int)uuid.getLeastSignificantBits()};
                        JsonObject state=new JsonObject();state.add("uuid",gson.toJsonTree(words));
                        state.addProperty("kind",net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()).toString());
                        state.add("pos",gson.toJsonTree(new double[]{entity.getX(),entity.getY(),entity.getZ()}));
                        state.addProperty("no_gravity",entity.isNoGravity());
                        JsonObject record=new JsonObject();record.addProperty("cmd","#projectile_saved_type "+op.get("label").getAsString());
                        record.add("messages",new JsonArray());record.add("results",new JsonArray());record.add("state",state);
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "raid_state" -> {
                        var raidLevel=player.level(); int id=op.get("id").getAsInt();
                        var raid=raidLevel.getRaids().get(id);
                        JsonObject state=new JsonObject(); state.addProperty("id",id); state.addProperty("present",raid!=null);
                        if (raid!=null) {
                            var tag=(net.minecraft.nbt.CompoundTag)net.minecraft.world.entity.raid.Raid.MAP_CODEC.codec()
                                .encodeStart(net.minecraft.nbt.NbtOps.INSTANCE,raid).getOrThrow();
                            state.addProperty("status",tag.getStringOr("status","missing"));
                            state.addProperty("started",raid.isStarted());state.addProperty("active",raid.isActive());
                            state.addProperty("groups_spawned",raid.getGroupsSpawned());
                            state.add("center",gson.toJsonTree(new int[]{raid.getCenter().getX(),raid.getCenter().getY(),raid.getCenter().getZ()}));
                            state.addProperty("village",raidLevel.isVillage(raid.getCenter()));
                            if (op.get("members").getAsBoolean()) state.addProperty("member_present",raid.getAllRaiders().stream().anyMatch(e->e.isAlive() && e.getCurrentRaid()==raid));
                        }
                        JsonObject raw=new JsonObject();raw.addProperty("label",op.get("label").getAsString());raw.addProperty("id",id);
                        var pp=op.getAsJsonArray("poi");var pos=new BlockPos(pp.get(0).getAsInt(),pp.get(1).getAsInt(),pp.get(2).getAsInt());
                        raw.addProperty("loaded",raidLevel.hasChunkAt(pos));
                        raw.addProperty("occupied",raidLevel.getPoiManager().getCountInRange(h->h.is(net.minecraft.tags.PoiTypeTags.VILLAGE),pos,0,net.minecraft.world.entity.ai.village.poi.PoiManager.Occupancy.IS_OCCUPIED));
                        raw.addProperty("poi_type",raidLevel.getPoiManager().getType(pos).map(h->h.unwrapKey().orElseThrow().identifier().toString()).orElse("none"));
                        raw.addProperty("raid_nbt",net.minecraft.world.entity.raid.Raids.CODEC.encodeStart(net.minecraft.nbt.NbtOps.INSTANCE,raidLevel.getRaids()).getOrThrow().toString());
                        JsonArray members=new JsonArray();if (raid!=null) for (var entity:raid.getAllRaiders()) members.add(raidActor(entity,raidLevel));raw.add("actors",members);
                        JsonArray villagers=new JsonArray();for (var uuid:op.getAsJsonArray("villagers")) {
                            var entity=raidLevel.getEntity(UUID.fromString(uuid.getAsString()));
                            if (!(entity instanceof net.minecraft.world.entity.npc.villager.Villager)) throw new IllegalStateException("raid fixture villager absent/wrong type");
                            villagers.add(raidActor((net.minecraft.world.entity.LivingEntity)entity,raidLevel));
                        }raw.add("villagers",villagers);
                        System.err.println("raid input raw: "+gson.toJson(raw));
                        JsonObject row=new JsonObject();row.addProperty("cmd","#raid_state "+op.get("label").getAsString()+" "+id);
                        row.add("messages",new JsonArray());row.add("results",new JsonArray());row.add("state",state);out.append(gson.toJson(row)).append('\n');
                    }
                    case "mob_anger" -> {
                        try {
                        var entity=player.level().getEntity(UUID.fromString(op.get("uuid").getAsString()));
                        if (entity==null || !entity.isAlive()) throw new IllegalStateException("anger state: missing live UUID");
                        String kind=net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()).toString();
                        if (!kind.equals(op.get("kind").getAsString())) throw new IllegalStateException("anger state: wrong actor type");
                        int index=op.get("index").getAsInt();
                        var field=entity.getClass().getDeclaredField("DATA_ANGER_END_TIME");field.setAccessible(true);
                        var accessor=(net.minecraft.network.syncher.EntityDataAccessor<?>)field.get(null);
                        if (accessor.id()!=index || accessor.serializer()!=net.minecraft.network.syncher.EntityDataSerializers.LONG)
                            throw new IllegalStateException("anger state: wrong actual metadata accessor");
                        long value=(Long)entity.getEntityData().get(accessor);
                        UUID uuid=entity.getUUID();
                        int[] words={(int)(uuid.getMostSignificantBits()>>32),(int)uuid.getMostSignificantBits(),
                            (int)(uuid.getLeastSignificantBits()>>32),(int)uuid.getLeastSignificantBits()};
                        JsonObject state=new JsonObject();state.add("uuid",gson.toJsonTree(words));state.addProperty("kind",kind);
                        state.addProperty("index",index);state.addProperty("serializer",net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(accessor.serializer()));
                        state.addProperty("value",value);state.addProperty("game_time",entity.level().getGameTime());
                        JsonObject record=new JsonObject();record.addProperty("cmd","#mob_anger "+op.get("label").getAsString());
                        record.add("messages",new JsonArray());record.add("results",new JsonArray());record.add("state",state);
                        out.append(gson.toJson(record)).append('\n');
                        } catch (ReflectiveOperationException e) {throw new IllegalStateException(e);}
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
                    case "boat_interact" -> {
                        var boat = level.getEntity(UUID.fromString(op.get("uuid").getAsString()));
                        if (!(boat instanceof net.minecraft.world.entity.vehicle.boat.AbstractBoat))
                            throw new IllegalStateException("boat click: missing live boat");
                        player.connection.handleInteract(new net.minecraft.network.protocol.game.ServerboundInteractPacket(
                            boat.getId(), net.minecraft.world.InteractionHand.MAIN_HAND,
                            net.minecraft.world.phys.Vec3.ZERO, false));
                    }
                    case "mob_interact" -> {
                        // The player's real Interact packet on a live mob, by UUID.
                        var mob = level.getEntity(UUID.fromString(op.get("uuid").getAsString()));
                        if (!(mob instanceof net.minecraft.world.entity.Mob) || !mob.isAlive())
                            throw new IllegalStateException("mob click: missing live mob");
                        player.connection.handleInteract(new net.minecraft.network.protocol.game.ServerboundInteractPacket(
                            mob.getId(), net.minecraft.world.InteractionHand.MAIN_HAND,
                            net.minecraft.world.phys.Vec3.ZERO, false));
                    }
                    case "use_item" -> {
                        if (!op.has("sequence")) {
                            player.gameMode.useItem(player, player.level(), player.getMainHandItem(),
                                net.minecraft.world.InteractionHand.MAIN_HAND);
                        } else {
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
                    }
                    case "crafter_max_map_fixture" -> {
                        // Independent official saved-map file, copied unchanged to the Game scratch world.
                        level.setMapData(new net.minecraft.world.level.saveddata.maps.MapId(31),
                            net.minecraft.world.level.saveddata.maps.MapItemSavedData.createFresh(
                                0, 0, (byte) 4, true, false, level.dimension()));
                        level.getServer().getDataStorage().saveAndJoin();
                    }
                    case "frame_map_state" -> {
                        int id = op.get("id").getAsInt();
                        var entity = level.getEntity(UUID.fromString(op.get("uuid").getAsString()));
                        if (!(entity instanceof net.minecraft.world.entity.decoration.ItemFrame frame)
                            || !net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(frame.getType())
                                .toString().equals(op.get("kind").getAsString()))
                            throw new IllegalStateException("frame map: missing/wrong live frame");
                        var map = net.minecraft.world.item.MapItem.getSavedData(
                            new net.minecraft.world.level.saveddata.maps.MapId(id), frame.level());
                        if (map == null) throw new IllegalStateException("frame map: no backed map");
                        var saved = (net.minecraft.nbt.CompoundTag)
                            net.minecraft.world.level.saveddata.maps.MapItemSavedData.CODEC
                                .encodeStart(net.minecraft.nbt.NbtOps.INSTANCE, map).getOrThrow();
                        var frames = saved.getList("frames").orElseGet(net.minecraft.nbt.ListTag::new);
                        net.minecraft.nbt.CompoundTag found = null;
                        for (var entry : frames) {
                            if (!(entry instanceof net.minecraft.nbt.CompoundTag tag))
                                throw new IllegalStateException("frame map: invalid saved frame");
                            if (tag.getInt("entity_id").orElseThrow() == frame.getId()) found = tag;
                        }
                        try {
                            var field = map.getClass().getDeclaredField("decorations");
                            field.setAccessible(true);
                            @SuppressWarnings("unchecked")
                            var decorations = (java.util.Map<String, net.minecraft.world.level.saveddata.maps.MapDecoration>)field.get(map);
                            var key = map.getClass().getDeclaredMethod("getFrameKey", int.class);
                            key.setAccessible(true);
                            var state = new JsonObject();
                            state.addProperty("id", id);
                            state.add("uuid", gson.toJsonTree(UUIDUtil.uuidToIntArray(frame.getUUID())));
                            state.addProperty("kind", op.get("kind").getAsString());
                            var p = frame.getPos();
                            state.add("pos", gson.toJsonTree(new int[]{p.getX(), p.getY(), p.getZ()}));
                            state.add("center", gson.toJsonTree(new int[]{map.centerX, map.centerZ}));
                            state.addProperty("scale", map.scale);
                            state.addProperty("dimension", map.dimension.identifier().toString());
                            state.addProperty("saved", found != null);
                            state.add("saved_pos", found == null ? com.google.gson.JsonNull.INSTANCE
                                : gson.toJsonTree(found.getIntArray("pos").orElseThrow()));
                            state.add("saved_rotation", found == null ? com.google.gson.JsonNull.INSTANCE
                                : gson.toJsonTree(found.getInt("rotation").orElseThrow()));
                            state.addProperty("decoration", decorations.containsKey((String)key.invoke(null, frame.getId())));
                            state.addProperty("frame_records", frames.size());
                            state.addProperty("frame_decorations", decorations.values().stream()
                                .filter(d -> d.type().equals(net.minecraft.world.level.saveddata.maps.MapDecorationTypes.FRAME)).count());
                            var record = new JsonObject();
                            record.addProperty("cmd", "#frame_map " + op.get("label").getAsString());
                            record.add("messages", new JsonArray());
                            record.add("results", new JsonArray());
                            record.add("state", state);
                            out.append(new com.google.gson.GsonBuilder().serializeNulls().create()
                                .toJson(record)).append('\n');
                        } catch (ReflectiveOperationException e) {
                            throw new IllegalStateException(e);
                        }
                    }
                    case "map_carried" -> {
                        // The harness has no player network listener; invoke the
                        // real map update normally reached from its player tick.
                        for (int i = 0; i < player.getInventory().getContainerSize(); i++) {
                            var stack = player.getInventory().getItem(i);
                            var id = stack.get(net.minecraft.core.component.DataComponents.MAP_ID);
                            if (id == null) continue;
                            var map = net.minecraft.world.item.MapItem.getSavedData(id, player.level());
                            if (map != null) map.tickCarriedBy(player, stack, null);
                        }
                    }
                    case "craft_menu_click" -> {
                        if (!(player.containerMenu instanceof net.minecraft.world.inventory.CraftingMenu))
                            throw new IllegalStateException("craft fixture: crafting menu not open");
                        int revision = player.containerMenu.getStateId();
                        System.err.println("craft fixture click: window=" + player.containerMenu.containerId + ", revision=" + revision);
                        player.connection.handleContainerClick(new net.minecraft.network.protocol.game.ServerboundContainerClickPacket(
                            player.containerMenu.containerId, revision, (short) op.get("slot").getAsInt(),
                            (byte) op.get("button").getAsInt(), net.minecraft.world.inventory.ContainerInput.values()[op.get("input").getAsInt()],
                            new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>(), net.minecraft.network.HashedStack.EMPTY));
                    }
                    case "craft_menu_close" -> {
                        player.connection.handleContainerClose(new net.minecraft.network.protocol.game.ServerboundContainerClosePacket(player.containerMenu.containerId));
                    }
                    case "craft_menu_state" -> {
                        if (!(player.containerMenu instanceof net.minecraft.world.inventory.CraftingMenu))
                            throw new IllegalStateException("craft fixture: crafting menu not open");
                        JsonObject state = new JsonObject();
                        state.addProperty("window", player.containerMenu.containerId);
                        state.addProperty("kind", net.minecraft.core.registries.BuiltInRegistries.MENU.getKey(player.containerMenu.getType()).toString());
                        JsonArray slots = new JsonArray();
                        for (var slot : player.containerMenu.slots) slots.add(craftStack(slot.getItem()));
                        state.add("slots", slots); state.add("carried", player.containerMenu.getCarried().isEmpty() ? new JsonArray() : craftStack(player.containerMenu.getCarried()));
                        JsonObject record = new JsonObject(); record.addProperty("cmd", "#craft_menu " + op.get("label").getAsString());
                        record.add("state", state); record.add("messages", new JsonArray()); record.add("results", new JsonArray());
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "map_files" -> {
                        this.getDataStorage().saveAndJoin();
                        Path directory = this.getWorldPath(net.minecraft.world.level.storage.LevelResource.ROOT).resolve("data/minecraft/maps");
                        JsonArray maps = new JsonArray();
                        for (var id : op.getAsJsonArray("ids")) {
                            Path path = directory.resolve(id.getAsInt() + ".dat");
                            if (!Files.exists(path)) { maps.add(com.google.gson.JsonNull.INSTANCE); continue; }
                            var root = net.minecraft.nbt.NbtIo.readCompressed(path, net.minecraft.nbt.NbtAccounter.unlimitedHeap());
                            var data = root.getCompoundOrEmpty("data");
                            JsonObject row = new JsonObject(); row.addProperty("scale", data.getIntOr("scale", 0));
                            row.addProperty("locked", data.getBooleanOr("locked", false));
                            JsonArray center = new JsonArray(); center.add(data.getIntOr("xCenter", 0)); center.add(data.getIntOr("zCenter", 0));
                            row.add("center", center); row.addProperty("dimension", data.getStringOr("dimension", ""));
                            row.addProperty("byte_array_length", data.getByteArray("colors").orElseThrow().length); maps.add(row);
                        }
                        var last = net.minecraft.nbt.NbtIo.readCompressed(directory.resolve("last_id.dat"), net.minecraft.nbt.NbtAccounter.unlimitedHeap());
                        JsonObject state = new JsonObject(); state.add("maps", maps); state.addProperty("last_id", last.getCompoundOrEmpty("data").getIntOr("map", -1));
                        JsonObject record = new JsonObject(); record.addProperty("cmd", "#map_files"); record.add("state", state);
                        record.add("messages", new JsonArray()); record.add("results", new JsonArray()); out.append(gson.toJson(record)).append('\n');
                    }
                    case "map_state" -> {
                        int id = op.get("id").getAsInt();
                        var map = net.minecraft.world.item.MapItem.getSavedData(
                            new net.minecraft.world.level.saveddata.maps.MapId(id), player.level());
                        String value = map == null ? "missing" : "scale=" + map.scale + ",locked=" + map.locked
                            + ",center=" + map.centerX + "," + map.centerZ;
                        if (map != null && op.has("red_x")) {
                            int targets = 0;
                            for (var decoration : map.getDecorations())
                                if (decoration.type().is(net.minecraft.world.level.saveddata.maps.MapDecorationTypes.RED_X)) targets++;
                            value += ",red_x=" + targets;
                        }
                        JsonObject record = new JsonObject();
                        record.addProperty("cmd", "#map_state " + id);
                        JsonArray messages = new JsonArray();
                        messages.add(spans(Component.literal(value)));
                        record.add("messages", messages);
                        record.add("results", new JsonArray());
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "light" -> {
                        JsonArray p = op.getAsJsonArray("pos");
                        BlockPos pos = new BlockPos(p.get(0).getAsInt(), p.get(1).getAsInt(), p.get(2).getAsInt());
                        // ThreadedLevelLightEngine runs the checkBlock tasks a /fill queues at most 1000 per update,
                        // so after a large fill the light lags by many ticks on a loaded runner (command_reinforcements
                        // read block light 10 in its sealed room, 2026-11-07). Read it once the tasks queued so far ran.
                        var pending = player.level().getChunkSource().getLightEngine().waitForPendingTasks(
                            pos.getX() >> 4, pos.getZ() >> 4);
                        long lightDeadline = System.nanoTime() + 120_000_000_000L;
                        while (!pending.isDone()) {
                            if (System.nanoTime() > lightDeadline) throw new IllegalStateException("light updates did not finish");
                            tick();
                            try { Thread.sleep(5); } catch (InterruptedException e) { throw new RuntimeException(e); }
                        }
                        JsonObject record = new JsonObject();
                        record.addProperty("cmd", "#light " + p);
                        record.add("messages", new JsonArray());
                        JsonArray results = new JsonArray();
                        for (var layer : new net.minecraft.world.level.LightLayer[]{
                            net.minecraft.world.level.LightLayer.BLOCK, net.minecraft.world.level.LightLayer.SKY}) {
                            JsonArray value = new JsonArray();
                            value.add(true);
                            value.add(player.level().getBrightness(layer, pos));
                            results.add(value);
                        }
                        record.add("results", results);
                        out.append(gson.toJson(record)).append('\n');
                    }
                    case "close_menu" -> player.closeContainer();
                    case "merchant_saved_stats" -> {
                        player.getStats().save();
                        String raw;
                        try {
                            var field=net.minecraft.stats.ServerStatsCounter.class.getDeclaredField("file");
                            field.setAccessible(true);
                            raw=Files.readString((Path)field.get(player.getStats()));
                        } catch(ReflectiveOperationException error) { throw new IllegalStateException(error); }
                        var saved=JsonParser.parseString(raw).getAsJsonObject().getAsJsonObject("stats");
                        JsonObject state=new JsonObject();
                        state.add("uuid",gson.toJsonTree(net.minecraft.core.UUIDUtil.uuidToIntArray(player.getUUID())));
                        String[][] keys={{"books","minecraft:crafted","minecraft:bookshelf"},
                            {"glass","minecraft:crafted","minecraft:glass"},
                            {"trades","minecraft:custom","minecraft:traded_with_villager"}};
                        for(var key:keys) {
                            var group=saved.getAsJsonObject(key[1]);
                            state.addProperty(key[0],group!=null&&group.has(key[2])?group.get(key[2]).getAsInt():0);
                        }
                        String label=op.get("label").getAsString();
                        Files.writeString(output.resolveSibling("java-merchant-stats-"+label+".json"),raw);
                        JsonObject row=new JsonObject();row.addProperty("cmd","#merchant_saved_stats "+label);
                        row.add("messages",new JsonArray());row.add("results",new JsonArray());row.add("state",state);
                        out.append(row.toString()).append('\n');
                    }
                    case "merchant_action" -> {
                        channel.runPendingTasks();channel.flushOutbound();channel.checkException();
                        if(!channel.isActive())throw new IllegalStateException("merchant channel inactive");
                        System.err.println("merchant action receipt: action="+op.get("action")+" player="+player.getUUID()
                            +" window="+player.containerMenu.containerId+" revision="+player.containerMenu.getStateId()
                            +" clock="+player.level().getGameTime());
                        switch(op.get("action").getAsString()) {
                            case "open" -> {
                                var entity=player.level().getEntity(UUID.fromString(op.get("uuid").getAsString()));
                                if(!(entity instanceof net.minecraft.world.entity.npc.villager.Villager))throw new IllegalStateException("missing real villager");
                                player.connection.handleInteract(new net.minecraft.network.protocol.game.ServerboundInteractPacket(
                                    entity.getId(),net.minecraft.world.InteractionHand.MAIN_HAND,new net.minecraft.world.phys.Vec3(0,0,0),false));
                            }
                            case "select" -> player.connection.handleSelectTrade(new net.minecraft.network.protocol.game.ServerboundSelectTradePacket(op.get("index").getAsInt()));
                            case "click" -> {
                                if(!(player.containerMenu instanceof net.minecraft.world.inventory.MerchantMenu))throw new IllegalStateException("no merchant menu");
                                player.connection.handleContainerClick(new net.minecraft.network.protocol.game.ServerboundContainerClickPacket(
                                    player.containerMenu.containerId,player.containerMenu.getStateId(),(short)op.get("slot").getAsInt(),
                                    (byte)(op.has("button")?op.get("button").getAsInt():0),
                                    net.minecraft.world.inventory.ContainerInput.values()[op.has("input")?op.get("input").getAsInt():0],
                                    new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>(),net.minecraft.network.HashedStack.EMPTY));
                            }
                            case "close" -> player.connection.handleContainerClose(new net.minecraft.network.protocol.game.ServerboundContainerClosePacket(player.containerMenu.containerId));
                            default -> throw new IllegalStateException("unknown merchant action");
                        }
                    }
                    case "merchant_state" -> {
                        if(!(player.containerMenu instanceof net.minecraft.world.inventory.MerchantMenu menu))throw new IllegalStateException("no merchant menu");
                        var trader=merchantOwner(menu);
                        JsonObject state=new JsonObject();state.addProperty("window",menu.containerId);
                        JsonArray uuid=new JsonArray(); for(int n:net.minecraft.core.UUIDUtil.uuidToIntArray(trader.getUUID()))uuid.add(n);state.add("merchant",uuid);
                        uuid=new JsonArray();for(int n:net.minecraft.core.UUIDUtil.uuidToIntArray(player.getUUID()))uuid.add(n);state.add("player",uuid);
                        JsonArray slots=new JsonArray();for(int n=0;n<3;n++)slots.add(merchantStack(menu.getSlot(n).getItem()));state.add("slots",slots);
                        state.add("carried",merchantStack(menu.getCarried()));state.addProperty("xp",menu.getTraderXp());
                        JsonArray offers=new JsonArray();for(var offer:menu.getOffers()) {
                            JsonObject o=new JsonObject();o.addProperty("uses",offer.getUses());o.addProperty("special",offer.getSpecialPriceDiff());
                            o.addProperty("cost",offer.getCostA().getCount());offers.add(o);
                        }state.add("offers",offers);
                        JsonObject record=new JsonObject();record.addProperty("cmd","#merchant_state "+op.get("label").getAsString());
                        record.add("messages",new JsonArray());record.add("results",new JsonArray());record.add("state",state);
                        out.append(record.toString()).append('\n');
                    }
                    case "menu_state" -> {
                        int slots = player.containerMenu == player.inventoryMenu ? 0 : player.containerMenu.slots.size() - 36;
                        JsonObject state = new JsonObject();state.addProperty("own_slots", slots);
                        JsonObject record = new JsonObject();record.addProperty("cmd", "#menu_state " + op.get("label").getAsString());
                        record.add("messages", new JsonArray());record.add("results", new JsonArray());record.add("state", state);
                        out.append(gson.toJson(record)).append('\n');
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
                    case "crafter_tick" -> {
                        JsonArray p = op.getAsJsonArray("pos");
                        BlockPos pos = new BlockPos(p.get(0).getAsInt(), p.get(1).getAsInt(), p.get(2).getAsInt());
                        var state = player.level().getBlockState(pos);
                        if (!state.is(net.minecraft.world.level.block.Blocks.CRAFTER) && !state.is(net.minecraft.world.level.block.Blocks.DISPENSER))
                            throw new IllegalStateException("crafter tick without a crafter or dispenser");
                        state.tick(player.level(), pos, player.level().getRandom());
                    }
                    case "sync_randoms" -> {
                        // docs/parity-workflow.md "Randoms that repeat": needs tools/seed_agent.py's -javaagent.
                        try {
                            Class.forName("dev.horizonlab.parity.SeedAgent").getMethod("setUniquifier", long.class)
                                .invoke(null, op.get("uniquifier").getAsLong());
                            var counter = ServerLevel.class.getDeclaredField("ENTITY_COUNTER");
                            counter.setAccessible(true);
                            ((java.util.concurrent.atomic.AtomicInteger) counter.get(null)).set(op.get("entity_id").getAsInt());
                        } catch (ReflectiveOperationException e) {
                            throw new IllegalStateException(e);
                        }
                        player.level().getRandom().setSeed(op.get("level_seed").getAsLong());
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

        static net.minecraft.world.entity.npc.villager.Villager merchantOwner(net.minecraft.world.inventory.MerchantMenu menu) {
            try {
                var field=net.minecraft.world.inventory.MerchantMenu.class.getDeclaredField("trader");field.setAccessible(true);
                return (net.minecraft.world.entity.npc.villager.Villager)field.get(menu);
            } catch(ReflectiveOperationException error) {throw new IllegalStateException(error);}
        }

        static com.google.gson.JsonElement merchantStack(net.minecraft.world.item.ItemStack stack) {
            if(stack.isEmpty())return com.google.gson.JsonNull.INSTANCE;
            JsonArray value=new JsonArray();value.add(net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(stack.getItem()).toString());
            value.add(stack.getCount());return value;
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

        static com.google.gson.JsonElement craftStack(net.minecraft.world.item.ItemStack stack) {
            if (stack.isEmpty()) return com.google.gson.JsonNull.INSTANCE;
            JsonArray value = new JsonArray();
            value.add(net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(stack.getItem()).toString()); value.add(stack.getCount());
            var map = stack.get(net.minecraft.core.component.DataComponents.MAP_ID);
            value.add(map == null ? null : map.id());
            var post = stack.get(net.minecraft.core.component.DataComponents.MAP_POST_PROCESSING);
            value.add(post == null ? null : post.id()); return value;
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
