from llm_client import LLMClient
import asyncio
async def main():
    client = LLMClient()
    message =[{'role':"user", "content": "hi"}]
    await client.chat_completion(message, False)
    print("kudos")

asyncio. run(main())