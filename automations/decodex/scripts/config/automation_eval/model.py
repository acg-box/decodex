"""Result model for automation checks."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class AutomationResult:
    automation_id: str
    status: str = "pass"
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    def fail(self, message: str) -> None:
        self.status = "fail"
        self.errors.append(message)

    def warn(self, message: str) -> None:
        self.warnings.append(message)
