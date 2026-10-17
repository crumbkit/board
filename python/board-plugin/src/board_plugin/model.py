"""The base of the pydantic models a plugin validates with beyond its schemas (STANDARDS.md P3)."""

from pydantic import BaseModel, ConfigDict


class Strict(BaseModel):
    """A document that refuses fields it does not know."""

    model_config = ConfigDict(extra="forbid", frozen=True, populate_by_name=True)
