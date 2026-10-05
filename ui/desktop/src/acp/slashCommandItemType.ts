import type { DisplayItem } from '../components/MentionPopover';

export type SlashCommandItemType = Extract<DisplayItem['itemType'], 'Builtin' | 'Recipe' | 'Skill'>;
