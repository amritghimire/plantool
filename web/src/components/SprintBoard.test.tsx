import {fireEvent,render,screen,waitFor} from "@testing-library/react";
import {MemoryRouter} from "react-router-dom";
import {api,type SprintBoardData} from "../api";
import {SprintBoard,sprintColumn} from "./SprintBoard";
const board:SprintBoardData={project:"p",repository:"o/r",current_week:"2026-W44",planning_queued:false,backlog_count:2,sprints:[{id:"s",week:"2026-W44",state:"waiting",issue:9,items:[{id:"i",issue:1,title:"Document tests",step:"plan_gate",membership:"pending",estimate:"Small",plan_hash:"hash",pr:null,frozen:false}]}]};
it("shows sprint issues and asks to replan without approving them",async()=>{
 vi.spyOn(api,"sprints").mockResolvedValue(board);const plan=vi.spyOn(api,"planSprint").mockResolvedValue({queued:true,week:board.current_week,message:"Planning is queued."});
 render(<MemoryRouter><SprintBoard project="p"/></MemoryRouter>);
 await screen.findByText("Document tests");expect(screen.getByRole("link",{name:"Review sprint on GitHub →"})).toHaveAttribute("href","https://github.com/o/r/issues/9");
 fireEvent.click(screen.getByRole("button",{name:"Replan this week with passkey"}));await waitFor(()=>expect(plan).toHaveBeenCalledWith("p",expect.any(String)));
});
it("offers planning when the week has no sprint",async()=>{
 vi.spyOn(api,"sprints").mockResolvedValue({...board,sprints:[]});render(<MemoryRouter><SprintBoard project="p"/></MemoryRouter>);
 await screen.findByText(/No sprint has been planned/);expect(screen.getByRole("button",{name:"Plan this week with passkey"})).toBeEnabled();
});
it("separates waiting, running and completed issues",()=>{
 const item=board.sprints[0].items[0];expect(sprintColumn(item)).toBe("Planned");expect(sprintColumn({...item,frozen:true})).toBe("Waiting");expect(sprintColumn({...item,step:"backend",membership:"started"})).toBe("In progress");expect(sprintColumn({...item,step:"merged"})).toBe("Done");
});

it("does not disable a stalled planning request or call it running",async()=>{
 vi.spyOn(api,"sprints").mockResolvedValue({...board,planning_queued:false,planning:{state:"stalled",message:"Planning has not started.",scheduler:"p:project"}});
 render(<MemoryRouter><SprintBoard project="p"/></MemoryRouter>);
 await screen.findByText("Planning has not started.");expect(screen.getByRole("button",{name:"Resume planning with passkey"})).toBeEnabled();expect(screen.queryByText("Planning queued or running")).not.toBeInTheDocument();
});
