import type { ModalType } from './modalType';
import type { ExtensionInfo } from './extensionInfo';

export interface ExtensionModalState {
  isOpen: boolean;
  modalType: ModalType;
  extensionInfo: ExtensionInfo | null;
  isPending: boolean;
  error: string | null;
}
