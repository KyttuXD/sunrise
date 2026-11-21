package dev.sunriselab.parity;

import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;
import java.util.UUID;
import net.minecraft.SharedConstants;
import net.minecraft.core.PositionAndRotation;
import net.minecraft.gametest.framework.GameTestServer;
import net.minecraft.network.Connection;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.network.protocol.game.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.CommonListenerCookie;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.entity.PersistentEntitySectionManager;
import net.minecraft.world.level.entity.Visibility;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.world.phys.Vec3;

/** Calls the real 26.3 play listener with synthetic serverbound packets on its server thread. */
public final class VanillaServerPlayerPacketsOracle {
    private static final Map<String, Object> rows = new LinkedHashMap<>();
    private static final Vec3 START = new Vec3(0.5, 100, 0.5);
    private static int nextProfile;

    private static final class Listener extends ServerGamePacketListenerImpl {
        final ArrayList<ClientboundPlayerPositionPacket> positions = new ArrayList<>();
        boolean disconnected;

        Listener(GameTestServer server, ServerPlayer player) {
            super(server, new Connection(PacketFlow.SERVERBOUND), player,
                CommonListenerCookie.createInitial(player.getGameProfile(), false));
        }

        @Override
        public void send(Packet<?> packet) {
            if (packet instanceof ClientboundPlayerPositionPacket position) positions.add(position);
        }

        @Override
        public void disconnect(Component reason) {
            disconnected = true;
        }
    }

    private static Field field(Class<?> type, String name) throws Exception {
        Field field = type.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

    private static Listener player(GameTestServer server, ServerLevel level, boolean loaded) {
        int n = ++nextProfile;
        ServerPlayer player = new ServerPlayer(server, level,
            new GameProfile(new UUID(0, n), "PacketPlayer" + n), ClientInformation.createDefault());
        Listener listener = new Listener(server, player);
        player.snapTo(START.x, START.y, START.z, 17, -12);
        player.setGameMode(GameType.SURVIVAL);
        if (loaded) new ServerboundPlayerLoadedPacket().handle(listener);
        return listener;
    }

    private static long[] pose(ServerPlayer player) {
        return new long[] {Double.doubleToRawLongBits(player.getX()), Double.doubleToRawLongBits(player.getY()),
            Double.doubleToRawLongBits(player.getZ()), Float.floatToRawIntBits(player.getYRot()),
            Float.floatToRawIntBits(player.getXRot())};
    }

    private static void credits(GameTestServer server) throws Exception {
        ServerLevel end = server.getLevel(Level.END);
        Listener listener = player(server, end, true);
        end.addNewPlayer(listener.player);
        listener.player.showEndCredits();
        new ServerboundMovePlayerPacket.PosRot(0.75, 100, 0.5, 60, 30, true, false).handle(listener);
        rows.put("141/credits_pose", pose(listener.player));
        rows.put("141/credits_move_count", field(ServerGamePacketListenerImpl.class, "receivedMovePacketCount").getInt(listener));
        new ServerboundMovePlayerPacket.Pos(0.75, 100, 0.5, true, false).handle(listener);
        rows.put("141/repeated_position", listener.disconnected);
        listener.disconnected = false;
        new ServerboundMovePlayerPacket.Rot(Float.NaN, 0, true, false).handle(listener);
        rows.put("141/invalid_rotation", listener.disconnected);
    }

    private static void useItem(GameTestServer server) throws Exception {
        WorldDataConfiguration original = server.getWorldData().getDataConfiguration();
        for (String kind : new String[] {"empty", "disabled", "survival", "spectator"}) {
            Listener listener = player(server, server.overworld(), true);
            if (!kind.equals("empty")) listener.player.setItemInHand(InteractionHand.OFF_HAND, new ItemStack(Items.SNOWBALL, 3));
            if (kind.equals("spectator")) listener.player.setGameMode(GameType.SPECTATOR);
            if (kind.equals("disabled")) server.getWorldData().setDataConfiguration(
                new WorldDataConfiguration(original.dataPacks(), FeatureFlagSet.of()));
            field(ServerPlayer.class, "lastActionTime").setLong(listener.player, 0);
            new ServerboundUseItemPacket(InteractionHand.OFF_HAND, 11, 725, -485).handle(listener);
            rows.put("142/" + kind, new int[] {Float.floatToRawIntBits(listener.player.getYRot()),
                Float.floatToRawIntBits(listener.player.getXRot()), listener.player.getOffhandItem().getCount(),
                field(ServerGamePacketListenerImpl.class, "ackBlockChangesUpTo").getInt(listener),
                listener.player.getLastActionTime() != 0 ? 1 : 0});
            server.getWorldData().setDataConfiguration(original);
        }
    }

    private static ServerboundPlayerInputPacket input(boolean shift) {
        return new ServerboundPlayerInputPacket(new Input(false, false, false, false, false, shift, false));
    }

    @SuppressWarnings("unchecked")
    private static void trackOrigin(ServerLevel level) throws Exception {
        // This fixture has no chunk tick loop. Mark the origin accessible so the real
        // entity section manager admits the test entities into the level's lookup.
        var manager = (PersistentEntitySectionManager<Entity>) field(ServerLevel.class, "entityManager").get(level);
        manager.updateChunkStatus(new ChunkPos(0, 0), Visibility.TRACKED);
    }

    private static void interact(GameTestServer server) {
        ServerLevel level = server.overworld();
        Listener listener = player(server, level, true);
        for (boolean shift : new boolean[] {true, false}) {
            input(!shift).handle(listener);
            new ServerboundInteractPacket(-1, InteractionHand.MAIN_HAND, Vec3.ZERO, shift).handle(listener);
            rows.put("143/missing_" + shift, listener.player.isShiftKeyDown());
        }
        for (boolean shift : new boolean[] {true, false}) {
            Listener rider = player(server, level, true);
            var boat = EntityTypes.OAK_BOAT.create(level, EntitySpawnReason.COMMAND);
            boat.setPos(1.5, 100, 0.5);
            if (!level.addFreshEntity(boat) || level.getEntity(boat.getUUID()) != boat) {
                throw new AssertionError("boat absent from level lookup");
            }
            input(!shift).handle(rider);
            new ServerboundInteractPacket(boat.getId(), InteractionHand.MAIN_HAND, Vec3.ZERO, shift).handle(rider);
            input(!shift).handle(rider);
            rows.put("143/boat_" + shift, rider.player.getVehicle() == boat);
            if (!shift) rows.put("143/next_input", rider.player.isShiftKeyDown());
        }
    }

    private static void spectate(GameTestServer server) {
        ServerLevel overworld = server.overworld(), nether = server.getLevel(Level.NETHER);
        Listener viewer = player(server, overworld, true);
        viewer.player.setGameMode(GameType.SPECTATOR);
        overworld.addNewPlayer(viewer.player);
        Listener target = player(server, overworld, true);
        target.player.snapTo(10.5, 104, 7.5, -35, 20);
        overworld.addNewPlayer(target.player);
        new ServerboundTeleportToEntityPacket(target.player.getUUID()).handle(viewer);
        rows.put("144/player", pose(viewer.player));
        viewer.positions.clear();
        new ServerboundTeleportToEntityPacket(new UUID(1, 1)).handle(viewer);
        rows.put("144/missing", viewer.positions.size());
        for (ServerLevel level : new ServerLevel[] {overworld, nether}) {
            var stand = EntityTypes.ARMOR_STAND.create(level, EntitySpawnReason.COMMAND);
            stand.snapTo(level == overworld ? 6.25 : 8.25, 101, 2.75, 63, 0);
            if (!level.addFreshEntity(stand) || level.getEntity(stand.getUUID()) != stand) {
                throw new AssertionError("stand absent from level lookup");
            }
            new ServerboundTeleportToEntityPacket(stand.getUUID()).handle(viewer);
            rows.put("144/stand_" + level.dimension().identifier().getPath(), pose(viewer.player));
            rows.put("144/level_" + level.dimension().identifier().getPath(), viewer.player.level() == level);
        }
    }

    private static void resend(GameTestServer server) throws Exception {
        Field tickCount = field(ServerGamePacketListenerImpl.class, "tickCount");
        for (String route : new String[] {"vehicle", "player", "loading_vehicle", "loading_player"}) {
            Listener listener = player(server, server.overworld(), !route.startsWith("loading_"));
            tickCount.setInt(listener, 100);
            listener.teleport(START.x, START.y, START.z, 17, -12);
            for (int elapsed : new int[] {19, 20, 21, 22}) {
                listener.positions.clear();
                tickCount.setInt(listener, 100 + elapsed);
                if (route.endsWith("vehicle")) {
                    new ServerboundMoveVehiclePacket(PositionAndRotation.of(new Vec3(2, 101, 3), 72, 35), true).handle(listener);
                } else {
                    new ServerboundMovePlayerPacket.Rot(72, 35, true, false).handle(listener);
                }
                var sent = listener.positions.isEmpty() ? null : listener.positions.getFirst();
                rows.put("145/" + route + "_" + elapsed,
                    new int[] {listener.positions.size(), sent == null ? -1 : sent.id(),
                        sent == null ? 0 : Float.floatToRawIntBits(sent.change().yRot()),
                        Float.floatToRawIntBits(listener.player.getYRot())});
            }
        }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        try (var access = LevelStorageSource.createDefault(Path.of(args[0])).createAccess("packet-oracle")) {
            var server = GameTestServer.create(Thread.currentThread(), access,
                ServerPacksSource.createPackRepository(access), Optional.empty(), false, 0);
            var init = GameTestServer.class.getDeclaredMethod("initServer");
            init.setAccessible(true);
            init.invoke(server);
            trackOrigin(server.overworld());
            trackOrigin(server.getLevel(Level.NETHER));
            credits(server);
            useItem(server);
            interact(server);
            spectate(server);
            resend(server);
            Files.writeString(Path.of(args[1]), new Gson().toJson(rows));
        }
        System.exit(0);
    }
}
