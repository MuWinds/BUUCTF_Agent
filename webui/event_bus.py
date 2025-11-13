"""Thread-safe publish/subscribe event bus for streaming agent updates."""
from __future__ import annotations

import queue
import threading
from typing import Any, Dict, Set


class EventBus:
    """Simple in-memory event bus bridging agent threads and async clients."""

    def __init__(self) -> None:
        self._subscribers: Set[queue.Queue] = set()
        self._lock = threading.Lock()

    def subscribe(self) -> queue.Queue:
        """Register a new subscriber and return its queue."""
        subscriber_queue: queue.Queue = queue.Queue()
        with self._lock:
            self._subscribers.add(subscriber_queue)
        return subscriber_queue

    def unsubscribe(self, subscriber_queue: queue.Queue) -> None:
        """Remove an existing subscriber if it is registered."""
        with self._lock:
            self._subscribers.discard(subscriber_queue)

    def publish(self, event: Dict[str, Any]) -> None:
        """Broadcast an event to every subscriber, ignoring queue backpressure."""
        with self._lock:
            subscribers = list(self._subscribers)

        for subscriber in subscribers:
            try:
                subscriber.put_nowait(event)
            except Exception:
                # Silently drop to keep the agent loop robust even if a client is gone.
                pass
