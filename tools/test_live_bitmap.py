"""Bounded bitmap transport and Python lifecycle tests (no native desktop needed)."""
from __future__ import annotations
import os
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'python'))
from ui4me_core.application import Application, StaleDocumentError
from ui4me_core.live_bitmap import LiveBitmap
from ui4me_core.client import RpcError


class BitmapRPC:
    def __init__(self, file):
        self.path = str(file)
        self.calls=[]
        self.handlers={}
        self.closing=False
        self.closing_error=None

    def on_notification(self, method, callback):
        self.handlers[method]=callback

    def on_request(self, method, callback):
        self.handlers[method]=callback

    def call(self, method, args=None, **kwargs):
        args=args or {}
        self.calls.append((method, args))
        if method=='window.create': return {'window_id':'w'}
        if method=='document.load': return {'document_id':'d'}
        if method=='bitmap.create':
            w,h,s=args['width'],args['height'],args['slots']
            with open(self.path,'wb') as f:f.truncate(w*h*4*s)
            return {'stride':w*4,'slot_bytes':w*h*4,'slots':s,'path':self.path}
        if method=='bitmap.present': return {'presented_to_engine':True,'sequence':args['sequence'],'slot_released':True}
        if method=='bitmap.close':
            if self.closing_error: raise RpcError(self.closing_error,'Application is shutting down or failed')
            return {'closed':True}
        return {}

    def close(self):self.closing=True


class LiveBitmapTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.rpc=BitmapRPC(Path(self.temp.name)/'bitmap')
        self.app=Application(self.rpc)
        self.window=self.app.open_html('<img src="ui4me-bitmap://plot">')
        self.doc=self.window.document
        self.addCleanup(self.app.close)

    def test_two_frames_are_written_to_separate_bounded_slots(self):
        bitmap=self.doc.create_bitmap('plot',4,3)
        first=bytes(range(48))
        second=bytes(reversed(range(48)))
        self.assertEqual(bitmap.present(first)['sequence'],1)
        self.assertEqual(bitmap.try_present(second)['sequence'],2)
        # The producer holds the transport delete-on-close on Windows, so a second
        # reader must share deletion (as the host's handle does).
        if os.name == 'nt':
            reader = os.fdopen(os.open(self.rpc.path, os.O_RDONLY | os.O_BINARY | os.O_TEMPORARY), 'rb')
        else:
            reader = open(self.rpc.path, 'rb')
        with reader as f:
            self.assertEqual(f.read(48),second)
            self.assertEqual(f.read(48),first)
        self.assertEqual([x[0] for x in self.rpc.calls[-3:]],['bitmap.create','bitmap.present','bitmap.present'])
        bitmap.close()
        self.assertEqual(self.rpc.calls[-1][0],'bitmap.close')

    def test_invalid_frame_does_not_enter_native_transport(self):
        bitmap=self.doc.create_bitmap('plot',2,2)
        before=len(self.rpc.calls)
        with self.assertRaises(ValueError):bitmap.present(b'bad')
        self.assertEqual(len(self.rpc.calls),before)

    def test_nonblocking_frame_drop(self):
        bitmap=self.doc.create_bitmap('plot',1,1)
        bitmap._lock.acquire()
        try:self.assertIsNone(bitmap.try_present(bytes(4)))
        finally:bitmap._lock.release()
        self.assertEqual(bitmap._sequence,0)

    def test_stale_document_closes_mapping_without_relying_on_host_rpc(self):
        bitmap=self.doc.create_bitmap('plot',1,1)
        self.window.load_html('<p>replaced</p>')
        self.assertTrue(bitmap._closed)
        with self.assertRaises(RuntimeError):bitmap.present(bytes(4))
        # Original bitmap.close is avoided after stale document invalidation.
        self.assertEqual([name for name,_ in self.rpc.calls].count('bitmap.close'),0)

    def test_creation_failure_releases_native_bitmap(self):
        # Host creation succeeded, but the local mapper is unavailable.
        with patch('ui4me_core.live_bitmap.mmap.mmap', side_effect=OSError('mmap unavailable')):
            with self.assertRaises(OSError):self.doc.create_bitmap('plot',2,2)
        self.assertEqual(self.rpc.calls[-1][0],'bitmap.close')

    def test_close_while_host_shuts_down_is_not_an_application_error(self):
        # Closing the last window shuts the host down; it releases the
        # window's bitmaps itself, so a late explicit close must not raise.
        for code in (-32008, -32004):
            bitmap=self.doc.create_bitmap('plot',1,1)
            self.rpc.closing_error=code
            bitmap.close()
            self.assertTrue(bitmap._closed)
            self.rpc.closing_error=None
        bitmap=self.doc.create_bitmap('plot',1,1)
        self.rpc.closing_error=-32602
        with self.assertRaises(RpcError):bitmap.close()

    def test_invalid_identifier_and_geometry_rejected_before_create(self):
        before=len(self.rpc.calls)
        for name,width,height,slots in [('',4,4,2),('bad/name',4,4,2),('plot',0,4,2),('plot',4,4,4)]:
            with self.assertRaises(ValueError):self.doc.create_bitmap(name,width,height,slots=slots)
        self.assertEqual(len(self.rpc.calls),before)


if __name__=='__main__':unittest.main()
