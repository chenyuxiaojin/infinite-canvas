export const CONVERSATION_PAGE_SIZE = 12;
export const CONVERSATION_PAGE_OVERLAP = 2;

type Message = { id: string; text: string };

// Keep the full persisted conversation separate from the small rendering page.
// An ID anchor keeps a history page stable when new streaming messages arrive.
export function conversationWindow<T extends Message>(messages: T[], endId: string | null) {
    const anchor = endId === null ? -1 : messages.findIndex((message) => message.id === endId);
    const end = anchor < 0 ? messages.length : anchor + 1;
    const start = Math.max(0, end - CONVERSATION_PAGE_SIZE);
    return { start, end, latest: endId === null || anchor < 0, messages: messages.slice(start, end) };
}

export function conversationMatches(messages: Message[], query: string) {
    const text = query.trim().toLocaleLowerCase();
    return text ? messages.flatMap((message, index) => message.text.toLocaleLowerCase().includes(text) ? [index] : []) : [];
}
