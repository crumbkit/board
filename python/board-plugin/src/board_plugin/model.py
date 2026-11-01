"""The base of the pydantic models a plugin validates with beyond its schemas (STANDARDS.md P3)."""

from typing import Any, override

from pydantic import BaseModel, ConfigDict
from pydantic.config import JsonDict
from pydantic.json_schema import GenerateJsonSchema


class Strict(BaseModel):
    """A document that refuses fields it does not know."""

    model_config = ConfigDict(extra="forbid", frozen=True, populate_by_name=True)


def unset(schema: JsonDict) -> None:
    """Leave out the `null` default of a value the caller may leave out."""
    schema.pop("default", None)


class Untitled(GenerateJsonSchema):
    """Schemas without property titles, as Board's shape rules want (docs/architecture/records/shapes.md 11)."""

    @override
    def field_title_should_be_set(self, schema: Any) -> bool:
        return False
