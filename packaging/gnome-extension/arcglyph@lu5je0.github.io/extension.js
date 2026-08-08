import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const IFACE = `
<node>
  <interface name="io.github.lu5je0.Arcglyph.Helper">
    <method name="GetFocus">
      <arg type="s" direction="out" name="app_id"/>
      <arg type="b" direction="out" name="fullscreen"/>
      <arg type="b" direction="out" name="cursor_inside"/>
    </method>
    <method name="TrailStart"/>
    <method name="TrailEnd"/>
  </interface>
</node>`;

const BUS_NAME = 'io.github.lu5je0.Arcglyph.Helper';
const OBJECT_PATH = '/io/github/lu5je0/Arcglyph/Helper';
const POLL_MS = 10;

export default class ArcglyphHelperExtension extends Extension {
    enable() {
        this._trail = null;
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._nameId = Gio.bus_own_name(
            Gio.BusType.SESSION, BUS_NAME, Gio.BusNameOwnerFlags.NONE,
            null, null, null);
    }

    disable() {
        this.TrailEnd();
        if (this._nameId) {
            Gio.bus_unown_name(this._nameId);
            this._nameId = 0;
        }
        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
    }

    GetFocus() {
        const w = global.display.focus_window;
        if (!w)
            return ['', false, false];
        const appId = w.get_wm_class() || w.get_gtk_application_id() || '';
        const fullscreen = w.is_fullscreen();
        const [px, py] = global.get_pointer();
        const r = w.get_frame_rect();
        const inside = px >= r.x && px < r.x + r.width &&
                       py >= r.y && py < r.y + r.height;
        return [appId, fullscreen, inside];
    }

    TrailStart() {
        this.TrailEnd();

        const area = new St.DrawingArea({reactive: false});
        area.set_position(0, 0);
        area.set_size(global.stage.width, global.stage.height);
        Main.layoutManager.uiGroup.add_child(area);
        // above everything, including fullscreen-adjacent chrome
        Main.layoutManager.uiGroup.set_child_above_sibling(area, null);

        const trail = {area, points: [], timeoutId: 0, repaintId: 0};
        trail.repaintId = area.connect('repaint', a => this._repaint(a, trail.points));
        trail.timeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, POLL_MS, () => {
            const [x, y] = global.get_pointer();
            const pts = trail.points;
            const last = pts.length > 0 ? pts[pts.length - 1] : null;
            if (!last || last[0] !== x || last[1] !== y) {
                pts.push([x, y]);
                trail.area.queue_repaint();
            }
            return GLib.SOURCE_CONTINUE;
        });
        this._trail = trail;
    }

    TrailEnd() {
        const trail = this._trail;
        if (!trail)
            return;
        this._trail = null;
        if (trail.timeoutId)
            GLib.source_remove(trail.timeoutId);
        if (trail.repaintId)
            trail.area.disconnect(trail.repaintId);
        trail.area.destroy();
    }

    _repaint(area, points) {
        if (points.length === 0)
            return;
        const cr = area.get_context();

        cr.setLineCap(1); // ROUND
        cr.setLineJoin(1); // ROUND

        const tracePath = () => {
            cr.newPath();
            cr.moveTo(points[0][0], points[0][1]);
            if (points.length === 2) {
                cr.lineTo(points[1][0], points[1][1]);
            } else {
                for (let i = 1; i < points.length - 1; i++) {
                    const [px, py] = points[i];
                    const [nx, ny] = points[i + 1];
                    cr.curveTo(px, py, px, py, (px + nx) / 2, (py + ny) / 2);
                }
                const last = points[points.length - 1];
                cr.lineTo(last[0], last[1]);
            }
        };

        // white halo
        for (const [width, alpha] of [[14, 0.10], [9, 0.22]]) {
            tracePath();
            cr.setSourceRGBA(1, 1, 1, alpha);
            cr.setLineWidth(width);
            cr.stroke();
        }
        // blue core
        tracePath();
        cr.setSourceRGBA(0x0a / 255, 0x84 / 255, 0xff / 255, 1);
        cr.setLineWidth(5);
        cr.stroke();

        // head cap: white dot rimmed in blue
        const [hx, hy] = points[points.length - 1];
        cr.newPath();
        cr.arc(hx, hy, 7.5, 0, 2 * Math.PI);
        cr.setSourceRGBA(1, 1, 1, 0.98);
        cr.fillPreserve();
        cr.setSourceRGBA(0x0a / 255, 0x84 / 255, 0xff / 255, 1);
        cr.setLineWidth(2);
        cr.stroke();

        cr.$dispose();
    }
}
