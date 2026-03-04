"""Resource catalog — defines all supply types, categories, and defaults."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class ResourceCategory(StrEnum):
    CONSUMABLE = "consumable"
    GEAR = "gear"


@dataclass(frozen=True)
class ResourceDef:
    key: str
    display: str          # 4-char UI label (FOOD, WATR, etc.)
    category: ResourceCategory
    default: int
    max_stack: int
    daily_decay: float    # Units lost per day (0 = none)
    warning_low: int      # UI warns below this
    description: str


# ── Resource catalog ─────────────────────────────────────────────────

RESOURCE_CATALOG: dict[str, ResourceDef] = {
    "food": ResourceDef(
        key="food", display="FOOD", category=ResourceCategory.CONSUMABLE,
        default=50, max_stack=200, daily_decay=0, warning_low=10,
        description="Daily rations. 2 per person per day, modified by pace.",
    ),
    "water": ResourceDef(
        key="water", display="WATR", category=ResourceCategory.CONSUMABLE,
        default=50, max_stack=200, daily_decay=0, warning_low=10,
        description="Water supply. 2 per person per day. Desert increases need.",
    ),
    "firewood": ResourceDef(
        key="firewood", display="FIRE", category=ResourceCategory.CONSUMABLE,
        default=15, max_stack=50, daily_decay=0, warning_low=3,
        description="Camp fuel. 1 per night. Cold weather uses more.",
    ),
    "meds": ResourceDef(
        key="meds", display="MEDS", category=ResourceCategory.CONSUMABLE,
        default=5, max_stack=20, daily_decay=0, warning_low=1,
        description="Medicine. Treats illness and injury.",
    ),
    "salt": ResourceDef(
        key="salt", display="SALT", category=ResourceCategory.CONSUMABLE,
        default=8, max_stack=20, daily_decay=0, warning_low=2,
        description="Preserves food. Without it, spoilage events increase.",
    ),
    "ammo": ResourceDef(
        key="ammo", display="AMMO", category=ResourceCategory.CONSUMABLE,
        default=20, max_stack=100, daily_decay=0, warning_low=5,
        description="Ammunition. Required for hunting and defense events.",
    ),
    "parts": ResourceDef(
        key="parts", display="PART", category=ResourceCategory.GEAR,
        default=3, max_stack=10, daily_decay=0, warning_low=1,
        description="Wagon parts. Required for repairs.",
    ),
    "rope": ResourceDef(
        key="rope", display="ROPE", category=ResourceCategory.GEAR,
        default=2, max_stack=5, daily_decay=0, warning_low=0,
        description="Rope. Helps with river crossings and rescue events.",
    ),
    "tools": ResourceDef(
        key="tools", display="TOOL", category=ResourceCategory.GEAR,
        default=1, max_stack=3, daily_decay=0, warning_low=0,
        description="Hand tools. Improves repair success.",
    ),
    "lantern_oil": ResourceDef(
        key="lantern_oil", display="LAMP", category=ResourceCategory.CONSUMABLE,
        default=6, max_stack=20, daily_decay=0, warning_low=2,
        description="Lantern fuel. 1 per night travel. Without it, danger increases.",
    ),
    "cloth": ResourceDef(
        key="cloth", display="CLTH", category=ResourceCategory.CONSUMABLE,
        default=5, max_stack=20, daily_decay=0, warning_low=1,
        description="Fabric for repairs and trade. Patches boots, wagon cover, harness.",
    ),
    "boots": ResourceDef(
        key="boots", display="BOOT", category=ResourceCategory.GEAR,
        default=2, max_stack=5, daily_decay=0, warning_low=1,
        description="Footwear. Worn out by rough terrain. Repair with cloth.",
    ),
}

DEFAULT_SUPPLIES: dict[str, int] = {
    rdef.key: rdef.default for rdef in RESOURCE_CATALOG.values()
}

# Legacy keys — the original 5 resources that existing code references directly
LEGACY_KEYS = {"food", "water", "meds", "ammo", "parts"}
