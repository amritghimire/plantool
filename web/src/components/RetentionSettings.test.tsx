import {fireEvent,render,screen,waitFor} from "@testing-library/react";
import {api} from "../api";
import {RetentionSettings} from "./RetentionSettings";
afterEach(()=>vi.restoreAllMocks());
it("loads only when opened and saves the selected retention policy",async()=>{
 const get=vi.spyOn(api,"retentionPolicy").mockResolvedValue({transcript_days:30,evidence_days:90,disk_cap_bytes:20*1024**3});
 const save=vi.spyOn(api,"saveRetentionPolicy").mockResolvedValue({ok:true});
 render(<RetentionSettings/>);expect(get).not.toHaveBeenCalled();
 const details=screen.getByText("Retention and disk use").closest("details")!;details.open=true;fireEvent(details,new Event("toggle"));
 await waitFor(()=>expect(screen.getByRole("button",{name:"Save retention with passkey"})).not.toBeDisabled());
 fireEvent.change(screen.getByLabelText("Transcript retention (days)"),{target:{value:"45"}});
 fireEvent.click(screen.getByRole("button",{name:"Save retention with passkey"}));
 await screen.findByText("Retention settings saved.");
 expect(save).toHaveBeenCalledWith({transcript_days:45,evidence_days:90,disk_cap_bytes:20*1024**3});
});
