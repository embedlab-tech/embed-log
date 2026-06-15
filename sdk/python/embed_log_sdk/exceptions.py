"""Custom exceptions for the embed-log SDK."""


class EmbedLogError(Exception):
    """Base exception for all embed-log SDK errors."""


class ConnectionError(EmbedLogError):
    """Raised when the connection to the server fails."""


class ProtocolError(EmbedLogError):
    """Raised when the server responds with an unexpected message."""


class ServerError(EmbedLogError):
    """Raised when the server returns an error response to a command."""

    def __init__(self, command: str, error: str, source_id: str = ""):
        self.command = command
        self.error = error
        self.source_id = source_id
        super().__init__(f"{command} failed on {source_id}: {error}")


class UnknownSourceError(ServerError):
    """Raised when an operation targets an unknown source."""


class NotWritableError(ServerError):
    """Raised when a TX write targets a non-writable source."""


class ConfigError(EmbedLogError):
    """Raised when the local config file cannot be parsed."""
