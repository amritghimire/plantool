import {fireEvent,render,screen,waitFor} from "@testing-library/react";
import {api} from "../api";
import {CleanBreakNotice} from "./CleanBreakNotice";
afterEach(()=>vi.restoreAllMocks());
it("shows the backup path and persists dismissal",async()=>{
 vi.spyOn(api,"cleanBreakNotice").mockResolvedValue({backup:"/tmp/plantool.v0-backup-123"});
 const dismiss=vi.spyOn(api,"dismissCleanBreakNotice").mockResolvedValue({ok:true});
 render(<CleanBreakNotice/>);
 expect(await screen.findByRole("status")).toHaveTextContent("/tmp/plantool.v0-backup-123");
 fireEvent.click(screen.getByRole("button",{name:"Dismiss backup notice"}));
 await waitFor(()=>expect(screen.queryByRole("status")).not.toBeInTheDocument());
 expect(dismiss).toHaveBeenCalledOnce();
});
