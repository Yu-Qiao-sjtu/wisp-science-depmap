"""Read-only MCP facade for the local DepMap 26Q1 knowledge base."""

from .server import DepMapEvidenceService, build_mcp_server

__all__ = ["DepMapEvidenceService", "build_mcp_server"]
