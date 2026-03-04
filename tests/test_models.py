"""Tests for core game state models."""

from escape_the_valley.models import (
    PartyMember,
    PartyState,
    SeededRNG,
    SuppliesState,
    Trait,
)


class TestSeededRNG:
    def test_determinism(self):
        """Same seed produces same sequence."""
        rng1 = SeededRNG(42)
        rng2 = SeededRNG(42)
        for _ in range(20):
            assert rng1.random() == rng2.random()

    def test_counter_resume(self):
        """Can resume from a counter position."""
        rng1 = SeededRNG(42)
        vals = [rng1.random() for _ in range(10)]

        # Resume from position 5
        rng2 = SeededRNG(42, counter=5)
        for i in range(5, 10):
            assert rng2.random() == vals[i]

    def test_randint(self):
        rng = SeededRNG(42)
        val = rng.randint(1, 100)
        assert 1 <= val <= 100

    def test_weighted_choice(self):
        rng = SeededRNG(42)
        items = ["a", "b", "c"]
        weights = [0.0, 0.0, 1.0]
        assert rng.weighted_choice(items, weights) == "c"


class TestSuppliesState:
    def test_apply_delta(self):
        s = SuppliesState(food=50, water=50, meds=5, ammo=20, parts=3)
        s.apply_delta({"food": -10, "water": -5, "meds": 2})
        assert s.food == 40
        assert s.water == 45
        assert s.meds == 7

    def test_no_negative(self):
        s = SuppliesState(food=5)
        s.apply_delta({"food": -100})
        assert s.food == 0

    def test_to_dict(self):
        s = SuppliesState(food=10, water=20, meds=3, ammo=5, parts=1)
        d = s.to_dict()
        assert d == {"food": 10, "water": 20, "meds": 3, "ammo": 5, "parts": 1}


class TestPartyState:
    def test_alive_count(self):
        p = PartyState(members=[
            PartyMember(name="A", health=50),
            PartyMember(name="B", health=0),
            PartyMember(name="C", health=80),
        ])
        assert p.alive_count == 2

    def test_avg_health(self):
        p = PartyState(members=[
            PartyMember(name="A", health=60),
            PartyMember(name="B", health=80),
        ])
        assert p.avg_health == 70

    def test_has_trait(self):
        p = PartyState(members=[
            PartyMember(name="A", traits=[Trait.HEALER]),
            PartyMember(name="B", traits=[Trait.TOUGH]),
        ])
        assert p.has_trait(Trait.HEALER)
        assert not p.has_trait(Trait.MECHANIC)

    def test_dead_member_no_trait(self):
        p = PartyState(members=[
            PartyMember(name="A", health=0, traits=[Trait.HEALER]),
        ])
        assert not p.has_trait(Trait.HEALER)
