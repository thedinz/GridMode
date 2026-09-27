import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";

const minScale = 1;
const maxScale = 8;
const wheelZoomFactor = 1.15;
const doubleClickScale = 2.5;

interface Transform {
  scale: number;
  x: number;
  y: number;
}

const identity: Transform = { scale: 1, x: 0, y: 0 };

export interface ZoomControls {
  zoomBy: (factor: number) => void;
  reset: () => void;
  isZoomed: () => boolean;
}

function clampScale(scale: number): number {
  return Math.min(maxScale, Math.max(minScale, scale));
}

/**
 * An image that fits its stage and can be zoomed with the wheel, a double
 * click, or the exposed controls, and panned by dragging while zoomed.
 */
export const ZoomableImage = forwardRef<
  ZoomControls,
  { src: string; alt: string; onZoomChange?: (zoomed: boolean) => void }
>(function ZoomableImage({ src, alt, onZoomChange }, ref) {
  const stageRef = useRef<HTMLDivElement | null>(null);
  const [transform, setTransform] = useState<Transform>(identity);
  const transformRef = useRef(transform);
  const dragRef = useRef<{ pointerId: number; startX: number; startY: number; origin: Transform }>();
  const [isDragging, setIsDragging] = useState(false);

  const apply = useCallback(
    (next: Transform) => {
      const stage = stageRef.current;
      // Keep at least part of the image in view: pan up to half the enlarged overflow.
      const limitX = stage ? (stage.clientWidth * (next.scale - 1)) / 2 : 0;
      const limitY = stage ? (stage.clientHeight * (next.scale - 1)) / 2 : 0;
      const clamped =
        next.scale <= minScale
          ? identity
          : {
              scale: next.scale,
              x: Math.max(-limitX, Math.min(limitX, next.x)),
              y: Math.max(-limitY, Math.min(limitY, next.y))
            };
      transformRef.current = clamped;
      setTransform(clamped);
      onZoomChange?.(clamped.scale > minScale);
    },
    [onZoomChange]
  );

  /** Zooms keeping the point under (clientX, clientY) fixed; defaults to the center. */
  const zoomAt = useCallback(
    (factor: number, clientX?: number, clientY?: number) => {
      const stage = stageRef.current;
      const current = transformRef.current;
      const scale = clampScale(current.scale * factor);
      if (!stage || scale === current.scale) {
        return;
      }
      const bounds = stage.getBoundingClientRect();
      const pointX = (clientX ?? bounds.left + bounds.width / 2) - (bounds.left + bounds.width / 2);
      const pointY = (clientY ?? bounds.top + bounds.height / 2) - (bounds.top + bounds.height / 2);
      const ratio = scale / current.scale;
      apply({
        scale,
        x: pointX - (pointX - current.x) * ratio,
        y: pointY - (pointY - current.y) * ratio
      });
    },
    [apply]
  );

  useImperativeHandle(
    ref,
    () => ({
      zoomBy: (factor) => zoomAt(factor),
      reset: () => apply(identity),
      isZoomed: () => transformRef.current.scale > minScale
    }),
    [apply, zoomAt]
  );

  useEffect(() => {
    apply(identity);
  }, [apply, src]);

  useEffect(() => {
    // React registers wheel listeners as passive, so preventDefault needs a native listener.
    const stage = stageRef.current;
    if (!stage) {
      return undefined;
    }
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      zoomAt(event.deltaY < 0 ? wheelZoomFactor : 1 / wheelZoomFactor, event.clientX, event.clientY);
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, [zoomAt]);

  const isZoomed = transform.scale > minScale;

  return (
    <div
      ref={stageRef}
      className={`zoom-stage${isZoomed ? " zoomed" : ""}${isDragging ? " dragging" : ""}`}
      onDoubleClick={(event) => {
        if (transformRef.current.scale > minScale) {
          apply(identity);
        } else {
          zoomAt(doubleClickScale, event.clientX, event.clientY);
        }
      }}
      onPointerDown={(event) => {
        if (!isZoomed || event.button !== 0) {
          return;
        }
        event.currentTarget.setPointerCapture(event.pointerId);
        dragRef.current = {
          pointerId: event.pointerId,
          startX: event.clientX,
          startY: event.clientY,
          origin: transformRef.current
        };
        setIsDragging(true);
      }}
      onPointerMove={(event) => {
        const drag = dragRef.current;
        if (!drag || drag.pointerId !== event.pointerId) {
          return;
        }
        apply({
          scale: drag.origin.scale,
          x: drag.origin.x + event.clientX - drag.startX,
          y: drag.origin.y + event.clientY - drag.startY
        });
      }}
      onPointerUp={() => {
        dragRef.current = undefined;
        setIsDragging(false);
      }}
      onPointerCancel={() => {
        dragRef.current = undefined;
        setIsDragging(false);
      }}
    >
      <img
        src={src}
        alt={alt}
        draggable={false}
        style={{ transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})` }}
      />
    </div>
  );
});
