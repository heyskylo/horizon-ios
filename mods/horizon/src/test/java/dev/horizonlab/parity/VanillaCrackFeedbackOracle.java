package dev.horizonlab.parity;

import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.network.protocol.game.ServerboundPlayerActionPacket.Action;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.ServerPlayerGameMode;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.block.state.BlockState;

/** Calls the pinned game mode's actual crack calculation, STOP and ABORT.
 * World storage, the player's range check and block destruction are controlled
 * providers; no server, connection, save, or game loop is started.
 */
public final class VanillaCrackFeedbackOracle {
    static final class State extends BlockState {
        float speed;
        State() { super(null, null, null); }
        @Override public boolean isAir() { return false; }
        @Override public float getDestroyProgress(Player player, BlockGetter world, BlockPos pos) {
            return speed;
        }
    }

    static final class Level extends ServerLevel {
        State state;
        List<Integer> stages;
        Level() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public BlockState getBlockState(BlockPos pos) { return state; }
        @Override public void destroyBlockProgress(int id, BlockPos pos, int stage) {
            stages.add(stage < 0 ? 255 : stage);
        }
    }

    static final class Observer extends ServerPlayer {
        Level world;
        Observer() { super(null, null, null, null); }
        @Override public ServerLevel level() { return world; }
        @Override public boolean isWithinBlockInteractionRange(BlockPos pos, double buffer) { return true; }
    }

    static final class Mode extends ServerPlayerGameMode {
        Mode(ServerPlayer player) { super(player); }
        @Override public boolean destroyBlock(BlockPos pos) { return true; }
    }

    static void set(ServerPlayerGameMode mode, String name, Object value) throws Exception {
        Field field = ServerPlayerGameMode.class.getDeclaredField(name);
        field.setAccessible(true);
        field.set(mode, value);
    }

    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var type = Class.forName("sun.misc.Unsafe");
        var access = type.getDeclaredField("theUnsafe");
        access.setAccessible(true);
        var unsafe = access.get(null);
        var allocate = type.getMethod("allocateInstance", Class.class);
        var level = (Level) allocate.invoke(unsafe, Level.class);
        var player = (Observer) allocate.invoke(unsafe, Observer.class);
        var state = (State) allocate.invoke(unsafe, State.class);
        level.state = state;
        level.stages = new ArrayList<>();
        player.world = level;
        player.setId(1);
        var mode = new Mode(player);
        var pos = new BlockPos(0, 100, 0);
        var increment = ServerPlayerGameMode.class.getDeclaredMethod(
            "incrementDestroyProgress", BlockState.class, BlockPos.class, int.class);
        increment.setAccessible(true);
        // The START branch uses this same stage calculation for the first hit.
        for (int tick = 0; tick <= 3; tick++) {
            state.speed = tick == 0 || tick == 2 ? 1.0f / 1.5f / 100.0f : 8.0f / 1.5f / 30.0f;
            increment.invoke(mode, state, pos, tick);
        }
        set(mode, "gameTicks", 103);
        set(mode, "destroyProgressStart", 100);
        set(mode, "destroyPos", pos);
        mode.handleBlockBreakAction(pos, Action.STOP_DESTROY_BLOCK, Direction.UP, 319, 1);
        state.speed = 1.0f / 1.5f / 100.0f;
        set(mode, "lastSentState", -1);
        increment.invoke(mode, state, pos, 0);
        mode.handleBlockBreakAction(pos, Action.ABORT_DESTROY_BLOCK, Direction.UP, 319, 1);
        Files.writeString(Path.of(args[0]), level.stages.toString());
    }
}
