"""Tests for world generation determinism."""

from escape_the_valley.models import Biome, GMProfile, SeededRNG
from escape_the_valley.worldgen import create_new_run, generate_map, generate_weather


class TestMapGeneration:
    def test_deterministic(self):
        """Same seed produces same map."""
        rng1 = SeededRNG(42)
        rng2 = SeededRNG(42)
        map1 = generate_map(rng1)
        map2 = generate_map(rng2)

        assert len(map1) == len(map2)
        for n1, n2 in zip(map1, map2, strict=False):
            assert n1.node_id == n2.node_id
            assert n1.name == n2.name
            assert n1.biome == n2.biome

    def test_has_nodes(self):
        rng = SeededRNG(42)
        nodes = generate_map(rng)
        assert len(nodes) >= 20

    def test_first_and_last_are_towns(self):
        rng = SeededRNG(42)
        nodes = generate_map(rng)
        assert nodes[0].is_town
        assert nodes[-1].is_town

    def test_connections_exist(self):
        rng = SeededRNG(42)
        nodes = generate_map(rng)
        # First node should have at least one connection
        assert len(nodes[0].connections) >= 1


class TestWeather:
    def test_deterministic(self):
        rng1 = SeededRNG(42)
        rng2 = SeededRNG(42)
        w1 = generate_weather(rng1, Biome.PLAINS, 5)
        w2 = generate_weather(rng2, Biome.PLAINS, 5)
        assert w1 == w2

    def test_returns_valid_weather(self):
        rng = SeededRNG(42)
        w = generate_weather(rng, Biome.FOREST, 1)
        assert w is not None


class TestCreateNewRun:
    def test_deterministic(self):
        """Same seed creates identical runs."""
        run1 = create_new_run(seed=42, gm_profile=GMProfile.FIRESIDE)
        run2 = create_new_run(seed=42, gm_profile=GMProfile.FIRESIDE)

        assert run1.run_id == run2.run_id
        assert run1.seed == run2.seed
        assert len(run1.party.members) == len(run2.party.members)
        for m1, m2 in zip(run1.party.members, run2.party.members, strict=False):
            assert m1.name == m2.name
            assert m1.traits == m2.traits

    def test_different_seeds_differ(self):
        run1 = create_new_run(seed=42)
        run2 = create_new_run(seed=99)
        assert run1.run_id != run2.run_id

    def test_has_party(self):
        run = create_new_run(seed=42)
        assert len(run.party.members) == 4
        assert all(m.is_alive() for m in run.party.members)

    def test_has_supplies(self):
        run = create_new_run(seed=42)
        assert run.supplies.food > 0
        assert run.supplies.water > 0

    def test_has_twists(self):
        run = create_new_run(seed=42)
        assert len(run.twists) >= 1
