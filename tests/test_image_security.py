from io import BytesIO
from types import SimpleNamespace
import socket
import pytest
from PIL import Image
from bot.services.images import ImageService,PublicResolver

class Content:
    def __init__(self,chunks):self.chunks=chunks
    async def iter_chunked(self,size):
        for chunk in self.chunks:yield chunk
class Response:
    def __init__(self,status=200,headers=None,chunks=()):self.status=status;self.headers=headers or {"Content-Type":"image/png"};self.content=Content(chunks)
    async def __aenter__(self):return self
    async def __aexit__(self,*args):pass
class Client:
    def __init__(self,responses):self.responses=iter(responses)
    async def __aenter__(self):return self
    async def __aexit__(self,*args):pass
    def get(self,*args,**kwargs):return next(self.responses)

def png_bytes():
    stream=BytesIO();Image.new("RGB",(2,2)).save(stream,"PNG");return stream.getvalue()

@pytest.mark.asyncio
async def test_fetch_reads_all_chunks_and_rejects_cumulative_oversize(monkeypatch):
    data=png_bytes();fake_http=SimpleNamespace(headers={});service=ImageService(fake_http,maximum=len(data)+1)
    monkeypatch.setattr("bot.services.images.aiohttp.ClientSession",lambda **kwargs:Client([Response(chunks=[data[:5],data[5:]])]))
    assert await service.fetch("https://example.com/image.png")==data
    service.maximum=5;monkeypatch.setattr("bot.services.images.aiohttp.ClientSession",lambda **kwargs:Client([Response(chunks=[b"123",b"456"])]))
    with pytest.raises(ValueError,match="too large"):await service.fetch("https://example.com/image.png")

@pytest.mark.asyncio
async def test_redirect_target_and_invalid_image_are_rejected(monkeypatch):
    service=ImageService(SimpleNamespace(headers={}))
    monkeypatch.setattr("bot.services.images.aiohttp.ClientSession",lambda **kwargs:Client([Response(302,{"Location":"http://127.0.0.1/private"}),Response(chunks=[png_bytes()])]))
    with pytest.raises(ValueError,match="public HTTPS"):await service.fetch("https://example.com/image.png")
    monkeypatch.setattr("bot.services.images.aiohttp.ClientSession",lambda **kwargs:Client([Response(chunks=[b"not an image"])]))
    with pytest.raises(Exception):await service.fetch("https://example.com/image.png")

@pytest.mark.asyncio
async def test_resolver_fails_closed_for_private_addresses(monkeypatch):
    async def private(*args,**kwargs):return [(socket.AF_INET,socket.SOCK_STREAM,6,"",("127.0.0.1",443))]
    monkeypatch.setattr("asyncio.BaseEventLoop.getaddrinfo",private)
    with pytest.raises(ValueError,match="private"):await PublicResolver().resolve("attacker.test",443)
