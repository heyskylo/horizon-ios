package dev.horizonlab.parity;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.UUID;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.Brain;
import net.minecraft.world.entity.ai.gossip.GossipContainer;
import net.minecraft.world.entity.ai.memory.NearestVisibleLivingEntities;
import net.minecraft.world.entity.ai.sensing.Sensing;
import net.minecraft.world.entity.npc.villager.Villager;
import net.minecraft.world.entity.npc.villager.VillagerData;
import net.minecraft.world.entity.npc.villager.VillagerType;
import net.minecraft.world.entity.npc.villager.VillagerProfession;
import net.minecraft.world.entity.ai.village.ReputationEventType;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.trading.ItemCost;
import net.minecraft.world.item.trading.MerchantOffer;
import net.minecraft.world.item.trading.MerchantOffers;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;

/** Actual reputation dispatch and lazy visibility on isolated entities; no server is started. */
public final class VanillaVillagerReputationOracle {
    static Object unsafe;
    static Method allocate;
    static Field field(Class<?> type, String name) throws Exception {
        var f = type.getDeclaredField(name);
        f.setAccessible(true);
        return f;
    }
    static <T> T make(Class<T> type) throws Exception {
        return type.cast(allocate.invoke(unsafe, type));
    }
    static final class World extends ServerLevel {
        int angry;
        World() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public void broadcastEntityEvent(Entity entity, byte event) {
            if (event == 13) angry++;
        }
    }
    static final class Person extends ServerPlayer {
        int sent;
        Person() { super(null, null, null, null); }
        @Override public void sendMerchantOffers(int window, MerchantOffers offers, int level, int xp, boolean progress, boolean restock) { sent++; }
    }
    static final class Menu extends AbstractContainerMenu {
        Menu() { super(null, 1); }
        @Override public ItemStack quickMoveStack(Player player, int slot) { return ItemStack.EMPTY; }
        @Override public boolean stillValid(Player player) { return true; }
    }
    static final class Actor extends Villager {
        World world;
        boolean living = true;
        boolean invisible;
        float cover;
        boolean discrete;
        Brain<Villager> brain;
        Sensing sensing;
        Player trader;
        MerchantOffers offers;
        Actor() { super(EntityTypes.VILLAGER, null); }
        @Override public Level level() { return world; }
        @Override public boolean isAlive() { return living; }
        @Override public boolean isSpectator() { return false; }
        @Override public boolean isInvisible() { return invisible; }
        @Override public boolean isDiscrete() { return discrete; }
        @Override public float getArmorCoverPercentage() { return cover; }
        @Override public ItemStack getItemBySlot(EquipmentSlot slot) { return ItemStack.EMPTY; }
        @Override public Player getTradingPlayer() { return trader; }
        @Override public MerchantOffers getOffers() { return offers; }
        @Override public VillagerData getVillagerData() {
            return new VillagerData(BuiltInRegistries.VILLAGER_TYPE.getOrThrow(VillagerType.PLAINS), BuiltInRegistries.VILLAGER_PROFESSION.getOrThrow(VillagerProfession.NONE), 1);
        }
        @Override public Brain<Villager> getBrain() { return brain; }
        @Override public Sensing getSensing() { return sensing; }
        @Override public boolean hasLineOfSight(Entity target) { return true; }
    }
    static Actor actor(World world, int id, double x, double z, boolean invisible) throws Exception {
        var actor = make(Actor.class);
        actor.world = world;
        actor.living = true;
        actor.invisible = invisible;
        actor.brain = new Brain<>();
        actor.sensing = new Sensing(actor);
        actor.offers = new MerchantOffers();
        field(Entity.class, "id").setInt(actor, id);
        field(Entity.class, "uuid").set(actor, new UUID(0, id));
        field(Entity.class, "position").set(actor, new Vec3(x, 1.0, z));
        field(Villager.class, "gossips").set(actor, new GossipContainer());
        return actor;
    }
    static String witnesses(NearestVisibleLivingEntities memory) {
        var ids = new ArrayList<Integer>();
        for (var entity : memory.findAll(e -> e instanceof Villager)) ids.add(entity.getId());
        return ids.toString().replace(" ", "");
    }
    static void bindComponents() throws Exception {
        var repo = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        repo.reload();
        repo.setSelected(List.of("vanilla"));
        try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA, repo.openAllSelected())) {
            var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
            var tags = net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(net.minecraft.server.RegistryLayer.STATIC));
            var context = net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLD), tags);
            var worldRegistries = net.minecraft.resources.RegistryDataLoader.load(resources, context, net.minecraft.resources.RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
            tags.forEach(net.minecraft.core.Registry.PendingTags::apply);
            var access = layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLD, worldRegistries).compositeAccess();
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(access).forEach(value -> value.apply());
        }
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        bindComponents();
        var u = Class.forName("sun.misc.Unsafe");
        unsafe = field(u, "theUnsafe").get(null);
        allocate = u.getMethod("allocateInstance", Class.class);
        var world = make(World.class);
        var victim = actor(world, 1, 0, 0, false);
        var a = make(Person.class);
        var b = make(Person.class);
        field(Entity.class, "uuid").set(a, new UUID(0, 11));
        field(Entity.class, "uuid").set(b, new UUID(0, 12));
        var mob = actor(world, 19, 2, 0, false);
        victim.setLastHurtByMob(a);
        victim.setLastHurtByMob(b);
        victim.setLastHurtByMob(a);
        victim.living = false;
        victim.setLastHurtByMob(mob);
        var near = actor(world, 2, 10, 0, false);
        var hidden = actor(world, 3, 10, 0, true);
        var diagonal = actor(world, 4, 15, 15, false);
        var close = actor(world, 6, 1.5, 0, true);
        var far = actor(world, 7, 20, 0, false);
        var memory = new NearestVisibleLivingEntities(world, victim, List.of(near, hidden, diagonal, close, far));
        try (var out = Files.newBufferedWriter(Path.of(args[0]))) {
            out.write("hurt " + victim.getPlayerReputation(a) + " " + victim.getPlayerReputation(b)
                + " " + victim.getGossips().getReputation(mob.getUUID(), type -> true) + " " + world.angry + "\n");
            out.write("witnesses " + witnesses(memory) + "\n");
            near.invisible = true;
            hidden.invisible = false;
            out.write("cached " + witnesses(memory) + "\n");
            // A fresh sensor memory recomputes visibility from the changed entities.
            memory = new NearestVisibleLivingEntities(world, victim, List.of(near, hidden, diagonal, close, far));
            out.write("fresh " + witnesses(memory) + "\n");
            var merchant = actor(world, 91, 0, 0, false);
            merchant.trader = a;
            merchant.offers.add(new MerchantOffer(new ItemCost(Items.EMERALD, 20), new ItemStack(Items.BREAD), 12, 1, 0.2F));
            a.containerMenu = new Menu();
            var effects = new HashMap<net.minecraft.core.Holder<net.minecraft.world.effect.MobEffect>, MobEffectInstance>();
            field(LivingEntity.class, "activeEffects").set(a, effects);
            effects.put(MobEffects.HERO_OF_THE_VILLAGE, new MobEffectInstance(MobEffects.HERO_OF_THE_VILLAGE, 400, 0));
            var refresh = Villager.class.getDeclaredMethod("updateSpecialPrices", Player.class);
            refresh.setAccessible(true);
            refresh.invoke(merchant, a);
            var prices = new ArrayList<Integer>();
            prices.add(merchant.offers.getFirst().getSpecialPriceDiff());
            effects.put(MobEffects.HERO_OF_THE_VILLAGE, new MobEffectInstance(MobEffects.HERO_OF_THE_VILLAGE, 400, 1));
            merchant.setLastHurtByMob(b);
            prices.add(merchant.offers.getFirst().getSpecialPriceDiff());
            merchant.setLastHurtByMob(a);
            prices.add(merchant.offers.getFirst().getSpecialPriceDiff());
            merchant.setLastHurtByMob(b);
            prices.add(merchant.offers.getFirst().getSpecialPriceDiff());
            merchant.onReputationEventFrom(ReputationEventType.VILLAGER_KILLED, mob);
            prices.add(merchant.offers.getFirst().getSpecialPriceDiff());
            merchant.onReputationEventFrom(ReputationEventType.VILLAGER_KILLED, a);
            prices.add(merchant.offers.getFirst().getSpecialPriceDiff());
            out.write("prices " + String.join(" ", prices.stream().map(Object::toString).toList()) + " " + a.sent + "\n");
            for (int i = 0; i < 7; i++) merchant.setLastHurtByMob(a);
            int cappedBefore = merchant.offers.getFirst().getSpecialPriceDiff();
            effects.put(MobEffects.HERO_OF_THE_VILLAGE, new MobEffectInstance(MobEffects.HERO_OF_THE_VILLAGE, 400, 2));
            int sentBefore = a.sent;
            merchant.setLastHurtByMob(a);
            out.write("cap " + merchant.getPlayerReputation(a) + " " + cappedBefore + " " + merchant.offers.getFirst().getSpecialPriceDiff() + " " + (a.sent - sentBefore) + "\n");
            effects.put(MobEffects.HERO_OF_THE_VILLAGE, new MobEffectInstance(MobEffects.HERO_OF_THE_VILLAGE, 400, 3));
            merchant.setLastHurtByMob(a);
            out.write("nested " + merchant.getPlayerReputation(a) + " " + merchant.offers.getFirst().getSpecialPriceDiff() + "\n");
        }
    }
}
