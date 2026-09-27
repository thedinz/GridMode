import { Copy, ExternalLink, FolderOpen, Maximize2 } from "lucide-react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode
} from "react";
import type { PhotoAsset } from "../../shared/types";
import { gridModeApi } from "../gridModeApi";
import { getErrorMessage } from "../lib/format";
import { copyText, revealLabel } from "../lib/platform";

interface PhotoActions {
  /** Opens a photo in the viewer; `siblings` is the list arrow keys move through. */
  openPhoto: (photo: PhotoAsset, siblings: PhotoAsset[]) => void;
  showMenu: (event: ReactMouseEvent, photo: PhotoAsset, siblings: PhotoAsset[]) => void;
  notify: (message: string) => void;
}

const PhotoActionsContext = createContext<PhotoActions | null>(null);

export function usePhotoActions(): PhotoActions {
  const actions = useContext(PhotoActionsContext);
  if (!actions) {
    throw new Error("usePhotoActions must be used inside PhotoActionsProvider");
  }
  return actions;
}

interface MenuState {
  x: number;
  y: number;
  photo: PhotoAsset;
  siblings: PhotoAsset[];
}

export function PhotoActionsProvider({
  onOpenPhoto,
  notify,
  children
}: {
  onOpenPhoto: (photo: PhotoAsset, siblings: PhotoAsset[]) => void;
  notify: (message: string) => void;
  children: ReactNode;
}): JSX.Element {
  const [menu, setMenu] = useState<MenuState>();

  const showMenu = useCallback((event: ReactMouseEvent, photo: PhotoAsset, siblings: PhotoAsset[]) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, photo, siblings });
  }, []);

  const actions = useMemo<PhotoActions>(
    () => ({ openPhoto: onOpenPhoto, showMenu, notify }),
    [notify, onOpenPhoto, showMenu]
  );

  const run = useCallback(
    (task: () => Promise<void>, successMessage?: string) => {
      setMenu(undefined);
      task()
        .then(() => {
          if (successMessage) {
            notify(successMessage);
          }
        })
        .catch((error) => notify(getErrorMessage(error)));
    },
    [notify]
  );

  return (
    <PhotoActionsContext.Provider value={actions}>
      {children}
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(undefined)}
          items={[
            {
              label: "Open",
              icon: <Maximize2 size={15} />,
              onSelect: () => {
                setMenu(undefined);
                onOpenPhoto(menu.photo, menu.siblings);
              }
            },
            {
              label: revealLabel,
              icon: <FolderOpen size={15} />,
              onSelect: () => run(() => gridModeApi.photo.reveal(menu.photo.path))
            },
            {
              label: "Open with default app",
              icon: <ExternalLink size={15} />,
              onSelect: () => run(() => gridModeApi.photo.open(menu.photo.path))
            },
            {
              label: "Copy path",
              icon: <Copy size={15} />,
              onSelect: () => run(() => copyText(menu.photo.path), "Path copied")
            }
          ]}
        />
      ) : null}
    </PhotoActionsContext.Provider>
  );
}

interface MenuItem {
  label: string;
  icon: ReactNode;
  onSelect: () => void;
}

function ContextMenu({
  x,
  y,
  items,
  onClose
}: {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}): JSX.Element {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState({ left: x, top: y });

  useEffect(() => {
    // Keep the menu on screen near the window edges.
    const menu = menuRef.current;
    if (!menu) {
      return;
    }
    const { width, height } = menu.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
      top: Math.max(8, Math.min(y, window.innerHeight - height - 8))
    });
    menu.querySelector("button")?.focus();
  }, [x, y]);

  useEffect(() => {
    const close = (event: Event) => {
      if (event instanceof KeyboardEvent && event.key !== "Escape") {
        return;
      }
      if (event.target instanceof Node && menuRef.current?.contains(event.target)) {
        return;
      }
      onClose();
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", close);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    window.addEventListener("wheel", onClose, { passive: true });
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", close);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("wheel", onClose);
    };
  }, [onClose]);

  return (
    <div
      ref={menuRef}
      className="context-menu"
      role="menu"
      style={position}
      onContextMenu={(event) => event.preventDefault()}
    >
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          onClick={item.onSelect}
        >
          {item.icon}
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  );
}
