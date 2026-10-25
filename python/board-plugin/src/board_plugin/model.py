"""The base of the pydantic models a plugin validates with beyond its schemas (STANDARDS.md P3)."""

from pydantic import BaseModel, ConfigDict
from pydantic.config import JsonDict


class Strict(BaseModel):
    """A document that refuses fields it does not know."""

    model_config = ConfigDict(extra="forbid", frozen=True, populate_by_name=True)


def unset(schema: JsonDict) -> None:
    """Leave out the `null` default of a value the caller may leave out."""
    schema.pop("default", None)
